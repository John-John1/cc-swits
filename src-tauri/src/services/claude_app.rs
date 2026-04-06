use crate::config::{get_app_config_dir, write_json_file};
use crate::database::Database;
use crate::provider::{ClaudeAppExactModelMappingEntry, ClaudeAppModelMapping, Provider};
use crate::services::ProxyService;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use tokio::sync::Mutex;

const CLAUDE_APP_CURRENT_KEY: &str = "claude_app";
const CLAUDE_PROVIDER_SOURCE_KEY: &str = "claude";
const PROXY_TOKEN_PLACEHOLDER: &str = "PROXY_MANAGED";
const WRAPPER_CONFIG_FILE: &str = "claude-app-wrapper.json";
const WRAPPER_BACKUP_EXE_NAME: &str = "claude.cc-switch-original.exe";
const WRAPPER_LOG_FILE: &str = "claude-app-wrapper.log";

#[derive(Debug, Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAppBridgeStatus {
    pub running: bool,
    pub provider_id: Option<String>,
    pub provider_name: Option<String>,
    pub proxy_base_url: Option<String>,
    pub proxy_messages_url: Option<String>,
    pub launch_command: Option<String>,
    pub pid: Option<u32>,
    pub started_at: Option<String>,
    pub message: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ClaudeAppBridgeRuntime {
    active: bool,
    provider_id: Option<String>,
    provider_name: Option<String>,
    target_model: Option<String>,
    started_at: Option<String>,
    last_error: Option<String>,
    installed_runtime_exe: Option<PathBuf>,
    backup_runtime_exe: Option<PathBuf>,
    proxy_base_url: Option<String>,
    proxy_messages_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeAppWrapperConfig {
    enabled: bool,
    provider_id: String,
    provider_name: String,
    target_model: String,
    model_mapping: ClaudeAppModelMapping,
    #[serde(default)]
    exact_model_mappings: Vec<ClaudeAppExactModelMappingEntry>,
    proxy_base_url: String,
    proxy_messages_url: String,
    runtime_exe: String,
    backup_runtime_exe: String,
    wrapper_source_exe: String,
    updated_at: String,
}

#[derive(Clone)]
pub struct ClaudeAppBridgeService {
    db: Arc<Database>,
    proxy_service: ProxyService,
    runtime: Arc<Mutex<ClaudeAppBridgeRuntime>>,
}

#[derive(Debug, Clone, Default)]
struct ResolvedClaudeAppMapping {
    family: ClaudeAppModelMapping,
    exact: HashMap<String, String>,
}

impl ClaudeAppBridgeService {
    pub fn new(db: Arc<Database>, proxy_service: ProxyService) -> Self {
        Self {
            db,
            proxy_service,
            runtime: Arc::new(Mutex::new(ClaudeAppBridgeRuntime::default())),
        }
    }

    pub fn set_app_handle(&self, _handle: tauri::AppHandle) {}

    pub async fn get_status(&self) -> Result<ClaudeAppBridgeStatus, String> {
        let mut runtime = self.runtime.lock().await.clone();
        if !runtime.active {
            if let Some(config) = Self::read_wrapper_config().filter(|cfg| cfg.enabled) {
                let _ = crate::settings::set_current_provider_for_key(
                    CLAUDE_APP_CURRENT_KEY,
                    Some(&config.provider_id),
                );
                runtime.active = true;
                runtime.provider_id = Some(config.provider_id.clone());
                runtime.provider_name = Some(config.provider_name.clone());
                runtime.target_model = Some(config.target_model.clone());
                runtime.started_at = Some(config.updated_at.clone());
                runtime.installed_runtime_exe = Some(PathBuf::from(config.runtime_exe.clone()));
                runtime.backup_runtime_exe =
                    Some(PathBuf::from(config.backup_runtime_exe.clone()));
                runtime.proxy_base_url = Some(config.proxy_base_url.clone());
                runtime.proxy_messages_url = Some(config.proxy_messages_url.clone());
            }
        }

        let provider = self.resolve_provider(
            runtime.provider_id.as_deref(),
            runtime.provider_name.as_deref(),
        )?;

        let message = if runtime.active {
            match (
                runtime.target_model.as_deref(),
                runtime.installed_runtime_exe.as_ref(),
            ) {
                (Some(model), Some(runtime_exe)) => format!(
                    "Wrapping official Claude App runtime at {} and rewriting local claude.exe launches to model {model} via {}.",
                    runtime_exe.display(),
                    runtime
                        .proxy_messages_url
                        .as_deref()
                        .unwrap_or("http://127.0.0.1:<port>/claude-app")
                ),
                _ => "Claude App takeover is active.".to_string(),
            }
        } else {
            "Select a provider to install the official Claude App local claude.exe wrapper and route launches through cc_switch."
                .to_string()
        };

        Ok(ClaudeAppBridgeStatus {
            running: runtime.active,
            provider_id: provider.as_ref().map(|item| item.id.clone()),
            provider_name: provider.as_ref().map(|item| item.name.clone()),
            proxy_base_url: runtime.proxy_base_url.clone(),
            proxy_messages_url: runtime.proxy_messages_url.clone(),
            launch_command: runtime
                .installed_runtime_exe
                .as_ref()
                .map(|path| path.display().to_string()),
            pid: None,
            started_at: runtime.started_at.clone(),
            message: Some(message),
            last_error: runtime.last_error.clone(),
        })
    }

    pub async fn activate_provider(
        &self,
        provider_id: &str,
    ) -> Result<ClaudeAppBridgeStatus, String> {
        let provider = self
            .db
            .get_provider_by_id(provider_id, CLAUDE_PROVIDER_SOURCE_KEY)
            .map_err(|e| format!("Failed to load Claude provider: {e}"))?
            .ok_or_else(|| format!("Provider not found: {provider_id}"))?;

        crate::settings::set_current_provider_for_key(CLAUDE_APP_CURRENT_KEY, Some(provider_id))
            .map_err(|e| format!("Failed to store Claude App provider selection: {e}"))?;

        let model_mapping = Self::resolve_model_mapping(&provider).ok_or_else(|| {
            format!(
                "Provider {} does not expose any Claude App takeover model mapping yet.",
                provider.name,
            )
        })?;
        let target_model = Self::primary_target_model(&model_mapping.family).ok_or_else(|| {
            format!(
                "Provider {} does not expose a usable Claude App takeover target model.",
                provider.name,
            )
        })?;

        let proxy_info = self.proxy_service.start().await?;
        let proxy_base_url = format!("http://{}:{}", proxy_info.address, proxy_info.port);
        let proxy_messages_url = format!("{proxy_base_url}/claude-app");
        let runtime_exe = Self::resolve_runtime_exe()?;
        let backup_runtime_exe = Self::backup_runtime_exe_path(&runtime_exe);
        let wrapper_source_exe =
            std::env::current_exe().map_err(|e| format!("Failed to resolve current exe: {e}"))?;

        Self::install_or_refresh_wrapper(&runtime_exe, &backup_runtime_exe, &wrapper_source_exe)?;

        let wrapper_config = ClaudeAppWrapperConfig {
            enabled: true,
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            target_model: target_model.clone(),
            model_mapping: model_mapping.family.clone(),
            exact_model_mappings: Self::exact_model_mappings_from_provider(&provider),
            proxy_base_url: proxy_base_url.clone(),
            proxy_messages_url: proxy_messages_url.clone(),
            runtime_exe: runtime_exe.display().to_string(),
            backup_runtime_exe: backup_runtime_exe.display().to_string(),
            wrapper_source_exe: wrapper_source_exe.display().to_string(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        Self::write_wrapper_config(&wrapper_config)?;

        {
            let mut runtime = self.runtime.lock().await;
            runtime.active = true;
            runtime.provider_id = Some(provider.id.clone());
            runtime.provider_name = Some(provider.name.clone());
            runtime.target_model = Some(target_model);
            runtime.started_at = Some(chrono::Utc::now().to_rfc3339());
            runtime.last_error = None;
            runtime.installed_runtime_exe = Some(runtime_exe);
            runtime.backup_runtime_exe = Some(backup_runtime_exe);
            runtime.proxy_base_url = Some(proxy_base_url);
            runtime.proxy_messages_url = Some(proxy_messages_url);
        }

        self.get_status().await
    }

    pub async fn stop(&self) -> Result<(), String> {
        let config_runtime = Self::read_wrapper_config();
        let (runtime_exe, backup_runtime_exe) = {
            let runtime = self.runtime.lock().await;
            (
                runtime
                    .installed_runtime_exe
                    .clone()
                    .or_else(|| config_runtime.as_ref().map(|cfg| PathBuf::from(&cfg.runtime_exe))),
                runtime.backup_runtime_exe.clone().or_else(|| {
                    config_runtime
                        .as_ref()
                        .map(|cfg| PathBuf::from(&cfg.backup_runtime_exe))
                }),
            )
        };

        if let (Some(runtime_exe), Some(backup_runtime_exe)) = (runtime_exe, backup_runtime_exe) {
            Self::restore_wrapper(&runtime_exe, &backup_runtime_exe)?;
        }

        let _ = std::fs::remove_file(Self::wrapper_config_path());

        let mut runtime = self.runtime.lock().await;
        runtime.active = false;
        runtime.started_at = None;
        runtime.target_model = None;
        runtime.last_error = None;
        runtime.installed_runtime_exe = None;
        runtime.backup_runtime_exe = None;
        runtime.proxy_base_url = None;
        runtime.proxy_messages_url = None;
        Ok(())
    }

    fn resolve_provider(
        &self,
        provider_id: Option<&str>,
        provider_name: Option<&str>,
    ) -> Result<Option<Provider>, String> {
        if let Some(id) = provider_id {
            return self
                .db
                .get_provider_by_id(id, CLAUDE_PROVIDER_SOURCE_KEY)
                .map_err(|e| format!("Failed to load Claude provider: {e}"));
        }

        if let Some(id) = crate::settings::get_current_provider_for_key(CLAUDE_APP_CURRENT_KEY) {
            return self
                .db
                .get_provider_by_id(&id, CLAUDE_PROVIDER_SOURCE_KEY)
                .map_err(|e| format!("Failed to load Claude provider: {e}"));
        }

        Ok(provider_name.map(|name| {
            Provider::with_id(String::new(), name.to_string(), serde_json::json!({}), None)
        }))
    }

    fn normalize_model_value(value: Option<String>) -> Option<String> {
        value.and_then(|item| {
            let trimmed = item.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    }

    fn normalize_model_key(value: Option<&str>) -> Option<String> {
        value
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| item.to_ascii_lowercase())
    }

    fn looks_like_claude_source_model(value: &str) -> bool {
        let normalized = value.trim().to_ascii_lowercase();
        !normalized.is_empty()
            && (normalized.starts_with("claude-")
                || normalized.contains("sonnet")
                || normalized.contains("opus")
                || normalized.contains("haiku"))
    }

    fn provider_env_model(provider: &Provider, key: &str) -> Option<String> {
        Self::normalize_model_value(
            provider
            .settings_config
            .get("env")
            .and_then(|env| env.get(key))
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
        )
    }

    fn primary_target_model(mapping: &ClaudeAppModelMapping) -> Option<String> {
        mapping
            .sonnet_model
            .clone()
            .or_else(|| mapping.default_model.clone())
            .or_else(|| mapping.opus_model.clone())
            .or_else(|| mapping.haiku_model.clone())
            .or_else(|| mapping.thinking_model.clone())
            .and_then(|value| Self::normalize_model_value(Some(value)))
    }

    fn exact_model_mappings_from_provider(provider: &Provider) -> Vec<ClaudeAppExactModelMappingEntry> {
        provider
            .meta
            .as_ref()
            .map(|meta| meta.claude_app_exact_model_mappings.clone())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| {
                let source_model = Self::normalize_model_value(Some(entry.source_model))?;
                let target_model = Self::normalize_model_value(Some(entry.target_model))?;
                Some(ClaudeAppExactModelMappingEntry {
                    source_model,
                    target_model,
                })
            })
            .collect()
    }

    fn resolved_mapping_from_parts(
        family: ClaudeAppModelMapping,
        exact_model_mappings: Vec<ClaudeAppExactModelMappingEntry>,
    ) -> Option<ResolvedClaudeAppMapping> {
        if family.default_model.is_none()
            && family.sonnet_model.is_none()
            && family.opus_model.is_none()
            && family.haiku_model.is_none()
            && family.thinking_model.is_none()
            && exact_model_mappings.is_empty()
        {
            return None;
        }

        Some(ResolvedClaudeAppMapping {
            family,
            exact: exact_model_mappings
                .into_iter()
                .filter_map(|entry| {
                    let key = Self::normalize_model_key(Some(&entry.source_model))?;
                    let value = Self::normalize_model_value(Some(entry.target_model))?;
                    Some((key, value))
                })
                .collect(),
        })
    }

    fn resolve_model_mapping(provider: &Provider) -> Option<ResolvedClaudeAppMapping> {
        let default_model = Self::provider_env_model(provider, "ANTHROPIC_MODEL")
            .or_else(|| Self::provider_env_model(provider, "ANTHROPIC_DEFAULT_SONNET_MODEL"))
            .or_else(|| Self::provider_env_model(provider, "ANTHROPIC_DEFAULT_OPUS_MODEL"))
            .or_else(|| Self::provider_env_model(provider, "ANTHROPIC_DEFAULT_HAIKU_MODEL"));

        let sonnet_model = Self::provider_env_model(provider, "ANTHROPIC_DEFAULT_SONNET_MODEL")
            .or_else(|| default_model.clone());
        let opus_model = Self::provider_env_model(provider, "ANTHROPIC_DEFAULT_OPUS_MODEL")
            .or_else(|| default_model.clone())
            .or_else(|| sonnet_model.clone());
        let haiku_model = Self::provider_env_model(provider, "ANTHROPIC_DEFAULT_HAIKU_MODEL")
            .or_else(|| default_model.clone())
            .or_else(|| sonnet_model.clone());
        let thinking_model = Self::provider_env_model(provider, "ANTHROPIC_REASONING_MODEL")
            .or_else(|| default_model.clone())
            .or_else(|| sonnet_model.clone());
        let exact_model_mappings = Self::exact_model_mappings_from_provider(provider);

        Self::resolved_mapping_from_parts(
            ClaudeAppModelMapping {
                default_model,
                haiku_model,
                sonnet_model,
                opus_model,
                thinking_model,
            },
            exact_model_mappings,
        )
    }

    fn request_uses_thinking(body: &Value) -> bool {
        body.get("thinking")
            .map(|value| !value.is_null())
            .unwrap_or(false)
            || body
                .pointer("/metadata/reasoning")
                .map(|value| !value.is_null())
                .unwrap_or(false)
    }

    fn target_model_from_mapping(
        mapping: &ResolvedClaudeAppMapping,
        requested_model: Option<&str>,
        uses_thinking: bool,
    ) -> Option<String> {
        let requested_model_raw = requested_model
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string);
        let requested_model_key =
            Self::normalize_model_key(requested_model_raw.as_deref());

        if let Some(requested_model) = requested_model_raw.as_deref() {
            if !Self::looks_like_claude_source_model(requested_model) {
                return Self::normalize_model_value(Some(requested_model.to_string()));
            }
        }

        if let Some(requested_model_key) = requested_model_key.as_deref() {
            if let Some(target_model) = mapping.exact.get(requested_model_key) {
                return Self::normalize_model_value(Some(target_model.clone()));
            }
        }

        if uses_thinking {
            if let Some(model) = mapping
                .family
                .thinking_model
                .clone()
                .and_then(|value| Self::normalize_model_value(Some(value)))
            {
                return Some(model);
            }
        }

        if let Some(requested_model) = requested_model_key.as_deref() {
            if requested_model.contains("haiku") {
                return mapping
                    .family
                    .haiku_model
                    .clone()
                    .or_else(|| mapping.family.default_model.clone())
                    .or_else(|| mapping.family.sonnet_model.clone())
                    .and_then(|value| Self::normalize_model_value(Some(value)));
            }

            if requested_model.contains("opus") {
                return mapping
                    .family
                    .opus_model
                    .clone()
                    .or_else(|| mapping.family.default_model.clone())
                    .or_else(|| mapping.family.sonnet_model.clone())
                    .and_then(|value| Self::normalize_model_value(Some(value)));
            }

            if requested_model.contains("sonnet") {
                return mapping
                    .family
                    .sonnet_model
                    .clone()
                    .or_else(|| mapping.family.default_model.clone())
                    .and_then(|value| Self::normalize_model_value(Some(value)));
            }
        }

        Self::primary_target_model(&mapping.family)
    }

    fn resolved_mapping_from_wrapper_config(
        config: &ClaudeAppWrapperConfig,
    ) -> Option<ResolvedClaudeAppMapping> {
        Self::resolved_mapping_from_parts(
            config.model_mapping.clone(),
            config.exact_model_mappings.clone(),
        )
    }

    fn dedupe_models(values: impl IntoIterator<Item = String>) -> Vec<String> {
        let mut result = Vec::new();
        let mut seen = HashMap::new();
        for value in values {
            let Some(normalized) = Self::normalize_model_value(Some(value)) else {
                continue;
            };
            let key = normalized.to_ascii_lowercase();
            if seen.contains_key(&key) {
                continue;
            }
            seen.insert(key, ());
            result.push(normalized);
        }
        result
    }

    fn update_provider_meta_lists<F>(
        db: &Database,
        provider_id: &str,
        mut updater: F,
    ) -> Result<Vec<String>, String>
    where
        F: FnMut(&mut crate::provider::ProviderMeta) -> Vec<String>,
    {
        let mut provider = db
            .get_provider_by_id(provider_id, CLAUDE_PROVIDER_SOURCE_KEY)
            .map_err(|e| format!("Failed to load Claude provider: {e}"))?
            .ok_or_else(|| format!("Provider not found: {provider_id}"))?;
        let mut meta = provider.meta.take().unwrap_or_default();
        let result = updater(&mut meta);
        provider.meta = Some(meta);
        db.save_provider(CLAUDE_PROVIDER_SOURCE_KEY, &provider)
            .map_err(|e| format!("Failed to save Claude provider: {e}"))?;
        Ok(result)
    }

    pub fn record_observed_source_model(
        db: &Database,
        provider_id: &str,
        model: &str,
    ) -> Result<Vec<String>, String> {
        let Some(model) = Self::normalize_model_value(Some(model.to_string())) else {
            return Self::get_observed_source_models(db, provider_id);
        };
        Self::update_provider_meta_lists(db, provider_id, |meta| {
            let models = Self::dedupe_models(
                meta.claude_app_observed_source_models
                    .iter()
                    .cloned()
                    .chain(std::iter::once(model.clone())),
            );
            meta.claude_app_observed_source_models = models.clone();
            models
        })
    }

    pub fn get_observed_source_models(
        db: &Database,
        provider_id: &str,
    ) -> Result<Vec<String>, String> {
        let provider = db
            .get_provider_by_id(provider_id, CLAUDE_PROVIDER_SOURCE_KEY)
            .map_err(|e| format!("Failed to load Claude provider: {e}"))?
            .ok_or_else(|| format!("Provider not found: {provider_id}"))?;
        Ok(Self::dedupe_models(
            provider
                .meta
                .unwrap_or_default()
                .claude_app_observed_source_models,
        ))
    }

    pub fn clear_observed_source_models(
        db: &Database,
        provider_id: &str,
    ) -> Result<Vec<String>, String> {
        Self::update_provider_meta_lists(db, provider_id, |meta| {
            meta.claude_app_observed_source_models.clear();
            Vec::new()
        })
    }

    pub fn set_fetched_target_models(
        db: &Database,
        provider_id: &str,
        models: Vec<String>,
    ) -> Result<Vec<String>, String> {
        let models = Self::dedupe_models(models);
        Self::update_provider_meta_lists(db, provider_id, |meta| {
            meta.claude_app_fetched_target_models = models.clone();
            models.clone()
        })
    }

    pub fn clear_fetched_target_models(
        db: &Database,
        provider_id: &str,
    ) -> Result<Vec<String>, String> {
        Self::update_provider_meta_lists(db, provider_id, |meta| {
            meta.claude_app_fetched_target_models.clear();
            Vec::new()
        })
    }

    fn resolve_runtime_exe() -> Result<PathBuf, String> {
        let appdata =
            std::env::var("APPDATA").map_err(|e| format!("Failed to resolve APPDATA: {e}"))?;
        let root = PathBuf::from(appdata).join("Claude").join("claude-code");
        let mut candidates: Vec<(Vec<u32>, PathBuf)> = Vec::new();

        let entries = std::fs::read_dir(&root)
            .map_err(|e| format!("Failed to read Claude runtime root {}: {e}", root.display()))?;

        for entry in entries {
            let entry =
                entry.map_err(|e| format!("Failed to inspect Claude runtime root: {e}"))?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let Some(version_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let runtime_exe = path.join("claude.exe");
            if runtime_exe.exists() {
                candidates.push((Self::parse_version(version_name), runtime_exe));
            }
        }

        candidates.sort_by(|a, b| a.0.cmp(&b.0));
        candidates
            .pop()
            .map(|(_, exe)| exe)
            .ok_or_else(|| format!("No official Claude App runtime found under {}", root.display()))
    }

    fn parse_version(raw: &str) -> Vec<u32> {
        raw.split('.')
            .map(|part| part.parse::<u32>().unwrap_or(0))
            .collect()
    }

    fn backup_runtime_exe_path(runtime_exe: &Path) -> PathBuf {
        runtime_exe
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(WRAPPER_BACKUP_EXE_NAME)
    }

    fn install_wrapper(
        runtime_exe: &Path,
        backup_runtime_exe: &Path,
        wrapper_source_exe: &Path,
    ) -> Result<(), String> {
        if !backup_runtime_exe.exists() {
            std::fs::rename(runtime_exe, backup_runtime_exe).map_err(|e| {
                format!(
                    "Failed to back up official Claude runtime {} -> {}: {e}",
                    runtime_exe.display(),
                    backup_runtime_exe.display()
                )
            })?;
        }

        std::fs::copy(wrapper_source_exe, runtime_exe).map_err(|e| {
            format!(
                "Failed to install cc_switch wrapper {} -> {}: {e}",
                wrapper_source_exe.display(),
                runtime_exe.display()
            )
        })?;
        Ok(())
    }

    fn install_or_refresh_wrapper(
        runtime_exe: &Path,
        backup_runtime_exe: &Path,
        wrapper_source_exe: &Path,
    ) -> Result<(), String> {
        if Self::installed_wrapper_matches_source(runtime_exe, backup_runtime_exe, wrapper_source_exe)
        {
            Self::append_wrapper_log(&format!(
                "Wrapper already matches source for {}. Skipping reinstall and only refreshing provider config.",
                runtime_exe.display()
            ));
            return Ok(());
        }

        match Self::install_wrapper(runtime_exe, backup_runtime_exe, wrapper_source_exe) {
            Ok(()) => Ok(()),
            Err(err) => {
                let is_sharing_violation = err.contains("os error 32");
                if !is_sharing_violation {
                    return Err(err);
                }

                Self::append_wrapper_log(&format!(
                    "Wrapper install hit sharing violation for {}. Attempting to stop running Claude local runtime processes.",
                    runtime_exe.display()
                ));

                Self::stop_running_runtime_processes(runtime_exe)?;

                match Self::install_wrapper(runtime_exe, backup_runtime_exe, wrapper_source_exe) {
                    Ok(()) => {
                        Self::append_wrapper_log(&format!(
                            "Wrapper install succeeded after stopping local Claude runtime processes for {}.",
                            runtime_exe.display()
                        ));
                        Ok(())
                    }
                    Err(retry_err) => {
                        if backup_runtime_exe.exists() {
                            Self::append_wrapper_log(&format!(
                                "Wrapper refresh still blocked for {}, but backup exists so provider config will continue using the installed wrapper. Error: {}",
                                runtime_exe.display(),
                                retry_err
                            ));
                            Ok(())
                        } else {
                            Err(retry_err)
                        }
                    }
                }
            }
        }
    }

    fn restore_wrapper(runtime_exe: &Path, backup_runtime_exe: &Path) -> Result<(), String> {
        if !backup_runtime_exe.exists() {
            return Ok(());
        }

        if runtime_exe.exists() {
            std::fs::remove_file(runtime_exe).map_err(|e| {
                format!(
                    "Failed to remove installed Claude wrapper {}: {e}",
                    runtime_exe.display()
                )
            })?;
        }

        std::fs::rename(backup_runtime_exe, runtime_exe).map_err(|e| {
            format!(
                "Failed to restore official Claude runtime {} -> {}: {e}",
                backup_runtime_exe.display(),
                runtime_exe.display()
            )
        })?;
        Ok(())
    }

    fn installed_wrapper_matches_source(
        runtime_exe: &Path,
        backup_runtime_exe: &Path,
        wrapper_source_exe: &Path,
    ) -> bool {
        if !runtime_exe.exists() || !backup_runtime_exe.exists() || !wrapper_source_exe.exists() {
            return false;
        }

        Self::same_file_contents(runtime_exe, wrapper_source_exe)
    }

    fn same_file_contents(left: &Path, right: &Path) -> bool {
        let Ok(left_meta) = std::fs::metadata(left) else {
            return false;
        };
        let Ok(right_meta) = std::fs::metadata(right) else {
            return false;
        };
        if left_meta.len() != right_meta.len() {
            return false;
        }

        let Ok(left_bytes) = std::fs::read(left) else {
            return false;
        };
        let Ok(right_bytes) = std::fs::read(right) else {
            return false;
        };
        left_bytes == right_bytes
    }

    fn wrapper_config_path() -> PathBuf {
        get_app_config_dir().join(WRAPPER_CONFIG_FILE)
    }

    fn read_wrapper_config() -> Option<ClaudeAppWrapperConfig> {
        std::fs::read_to_string(Self::wrapper_config_path())
            .ok()
            .and_then(|raw| serde_json::from_str::<ClaudeAppWrapperConfig>(&raw).ok())
    }

    fn write_wrapper_config(config: &ClaudeAppWrapperConfig) -> Result<(), String> {
        write_json_file(&Self::wrapper_config_path(), config)
            .map_err(|e| format!("Failed to write Claude App wrapper config: {e}"))
    }

    fn wrapper_log_path() -> PathBuf {
        get_app_config_dir().join("logs").join(WRAPPER_LOG_FILE)
    }

    fn append_wrapper_log(message: &str) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let line = format!("[{timestamp}] {message}\n");
        let path = Self::wrapper_log_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    #[cfg(windows)]
    fn stop_running_runtime_processes(runtime_exe: &Path) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const DETACHED_PROCESS: u32 = 0x00000008;

        let escaped_path = runtime_exe
            .display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('\'', "''");

        let script = format!(
            "$target = '{escaped_path}'; \
            Get-CimInstance Win32_Process | \
            Where-Object {{ $_.Name -ieq 'claude.exe' -and $_.ExecutablePath -and $_.ExecutablePath -ieq $target }} | \
            ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction Stop }}"
        );

        let powershell_exe = std::env::var("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"C:\Windows"))
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");

        let status = Command::new(&powershell_exe)
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-WindowStyle")
            .arg("Hidden")
            .arg("-Command")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .status()
            .map_err(|e| format!("Failed to stop running Claude local runtime process: {e}"))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "Failed to stop running Claude local runtime process for {}",
                runtime_exe.display()
            ))
        }
    }

    #[cfg(not(windows))]
    fn stop_running_runtime_processes(_runtime_exe: &Path) -> Result<(), String> {
        Ok(())
    }
}

pub fn active_wrapper_provider_id() -> Option<String> {
    ClaudeAppBridgeService::read_wrapper_config()
        .filter(|config| config.enabled)
        .map(|config| config.provider_id)
}

pub fn active_wrapper_model_mapping() -> Option<ClaudeAppModelMapping> {
    ClaudeAppBridgeService::read_wrapper_config()
        .filter(|config| config.enabled)
        .map(|config| config.model_mapping)
}

pub fn active_wrapper_target_model() -> Option<String> {
    ClaudeAppBridgeService::read_wrapper_config()
        .filter(|config| config.enabled)
        .map(|config| config.target_model)
}

pub fn active_wrapper_target_model_for_body(body: &Value) -> Option<String> {
    let config = ClaudeAppBridgeService::read_wrapper_config().filter(|config| config.enabled)?;
    let mapping = ClaudeAppBridgeService::resolved_mapping_from_wrapper_config(&config)?;
    let requested_model = body.get("model").and_then(|model| model.as_str());
    let uses_thinking = ClaudeAppBridgeService::request_uses_thinking(body);
    ClaudeAppBridgeService::target_model_from_mapping(&mapping, requested_model, uses_thinking)
}

pub fn maybe_run_claude_wrapper() -> bool {
    let Ok(current_exe) = std::env::current_exe() else {
        return false;
    };

    if !current_exe
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("claude.exe"))
        .unwrap_or(false)
    {
        return false;
    }

    let backup_exe = ClaudeAppBridgeService::backup_runtime_exe_path(&current_exe);
    if !backup_exe.exists() {
        return false;
    }

    let wrapper_config = ClaudeAppBridgeService::read_wrapper_config();

    let mut args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if let Some(config) = wrapper_config.as_ref().filter(|cfg| cfg.enabled) {
        let resolved_mapping = ClaudeAppBridgeService::resolved_mapping_from_wrapper_config(config);
        ClaudeAppBridgeService::append_wrapper_log(&format!(
            "Wrapper launch intercepted for provider={} target_model={} runtime={} raw_args={:?}",
            config.provider_name,
            config.target_model,
            current_exe.display(),
            args
        ));
        args = rewrite_model_args(&args, resolved_mapping.as_ref());
        ClaudeAppBridgeService::append_wrapper_log(&format!(
            "Rewrote Claude App launch args for provider={} rewritten_args={:?}",
            config.provider_name, args
        ));
    } else {
        ClaudeAppBridgeService::append_wrapper_log(&format!(
            "Wrapper launch intercepted without active config runtime={} raw_args={:?}",
            current_exe.display(),
            args
        ));
    }

    let mut command = Command::new(&backup_exe);
    command.args(&args);
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    command.env("CC_SWITCH_CLAUDE_WRAPPER_ACTIVE", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    if let Some(config) = wrapper_config.as_ref().filter(|cfg| cfg.enabled) {
        let resolved_mapping =
            ClaudeAppBridgeService::resolved_mapping_from_wrapper_config(config);
        let default_model = resolved_mapping
            .as_ref()
            .and_then(|mapping| {
                ClaudeAppBridgeService::target_model_from_mapping(
                    mapping,
                    Some("claude-sonnet-4-6"),
                    false,
                )
                .or_else(|| ClaudeAppBridgeService::primary_target_model(&mapping.family))
            })
            .unwrap_or_else(|| config.target_model.clone());
        let haiku_model = resolved_mapping
            .as_ref()
            .and_then(|mapping| {
                ClaudeAppBridgeService::target_model_from_mapping(
                    mapping,
                    Some("claude-haiku-4-5"),
                    false,
                )
            })
            .unwrap_or_else(|| default_model.clone());
        let opus_model = resolved_mapping
            .as_ref()
            .and_then(|mapping| {
                ClaudeAppBridgeService::target_model_from_mapping(
                    mapping,
                    Some("claude-opus-4-6"),
                    false,
                )
            })
            .unwrap_or_else(|| default_model.clone());
        let thinking_model = resolved_mapping
            .as_ref()
            .and_then(|mapping| {
                ClaudeAppBridgeService::target_model_from_mapping(
                    mapping,
                    Some("claude-sonnet-4-6"),
                    true,
                )
            })
            .unwrap_or_else(|| default_model.clone());
        command.env("CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST", "1");
        command.env("ANTHROPIC_BASE_URL", &config.proxy_messages_url);
        command.env("ANTHROPIC_AUTH_TOKEN", PROXY_TOKEN_PLACEHOLDER);
        command.env("ANTHROPIC_API_KEY", PROXY_TOKEN_PLACEHOLDER);
        command.env("ANTHROPIC_MODEL", &default_model);
        command.env("ANTHROPIC_DEFAULT_SONNET_MODEL", &default_model);
        command.env("ANTHROPIC_DEFAULT_HAIKU_MODEL", &haiku_model);
        command.env("ANTHROPIC_DEFAULT_OPUS_MODEL", &opus_model);
        command.env("ANTHROPIC_REASONING_MODEL", &thinking_model);
        command.env("CC_SWITCH_CLAUDE_APP_PROVIDER_ID", &config.provider_id);
    }

    let status = match command.status() {
        Ok(status) => status,
        Err(err) => {
            ClaudeAppBridgeService::append_wrapper_log(&format!(
                "Failed to launch original Claude runtime {}: {err}",
                backup_exe.display()
            ));
            eprintln!(
                "Failed to launch original Claude runtime {}: {err}",
                backup_exe.display()
            );
            std::process::exit(1);
        }
    };

    ClaudeAppBridgeService::append_wrapper_log(&format!(
        "Original Claude runtime exited with status={}",
        status.code().unwrap_or(1)
    ));

    std::process::exit(status.code().unwrap_or(1));
}

fn rewrite_model_args(
    args: &[OsString],
    mapping: Option<&ResolvedClaudeAppMapping>,
) -> Vec<OsString> {
    let mut rewritten = Vec::with_capacity(args.len() + 2);
    let mut i = 0usize;
    let mut replaced = false;

    while i < args.len() {
        let arg = &args[i];
        if arg == OsStr::new("--model") {
            let requested_model = args
                .get(i + 1)
                .and_then(|value| value.to_str())
                .map(ToString::to_string);
            let target_model = mapping.and_then(|resolved| {
                ClaudeAppBridgeService::target_model_from_mapping(
                    resolved,
                    requested_model.as_deref(),
                    false,
                )
                .or_else(|| ClaudeAppBridgeService::primary_target_model(&resolved.family))
            });
            let Some(target_model) = target_model else {
                i += 2;
                continue;
            };
            rewritten.push(OsString::from("--model"));
            rewritten.push(OsString::from(target_model));
            replaced = true;
            i += 2;
            continue;
        }

        if let Some(value) = arg.to_str() {
            if value.starts_with("--model=") {
                let requested_model = Some(value["--model=".len()..].to_string());
                let target_model = mapping.and_then(|resolved| {
                    ClaudeAppBridgeService::target_model_from_mapping(
                        resolved,
                        requested_model.as_deref(),
                        false,
                    )
                    .or_else(|| ClaudeAppBridgeService::primary_target_model(&resolved.family))
                });
                if let Some(target_model) = target_model {
                    rewritten.push(OsString::from(format!("--model={target_model}")));
                    replaced = true;
                } else {
                    rewritten.push(arg.clone());
                }
                i += 1;
                continue;
            }
        }

        rewritten.push(arg.clone());
        i += 1;
    }

    if !replaced {
        let target_model =
            mapping.and_then(|resolved| ClaudeAppBridgeService::primary_target_model(&resolved.family));
        let Some(target_model) = target_model else {
            return rewritten;
        };
        rewritten.push(OsString::from("--model"));
        rewritten.push(OsString::from(target_model));
    }

    rewritten
}

#[cfg(test)]
mod tests {
    use super::{rewrite_model_args, ClaudeAppBridgeService, ResolvedClaudeAppMapping};
    use crate::provider::{
        ClaudeAppExactModelMappingEntry, ClaudeAppModelMapping, Provider, ProviderMeta,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::ffi::OsString;

    #[test]
    fn resolve_model_mapping_prefers_provider_env_values() {
        let mut provider = Provider::with_id(
            "codex-auto".to_string(),
            "Codex Auto".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "gpt-5.4",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5.4",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.4-mini",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "o3",
                    "ANTHROPIC_REASONING_MODEL": "o4-mini"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            claude_app_model_mapping: Some(ClaudeAppModelMapping {
                sonnet_model: Some("gpt-5.5".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        });

        let mapping = ClaudeAppBridgeService::resolve_model_mapping(&provider).unwrap();
        assert_eq!(mapping.family.sonnet_model.as_deref(), Some("gpt-5.4"));
        assert_eq!(mapping.family.haiku_model.as_deref(), Some("gpt-5.4-mini"));
        assert_eq!(mapping.family.opus_model.as_deref(), Some("o3"));
        assert_eq!(mapping.family.thinking_model.as_deref(), Some("o4-mini"));
    }

    #[test]
    fn rewrite_model_args_replaces_existing_flag() {
        let args = vec![
            OsString::from("--output-format"),
            OsString::from("stream-json"),
            OsString::from("--model"),
            OsString::from("claude-sonnet-4-6"),
        ];

        let rewritten = rewrite_model_args(
            &args,
            Some(&ResolvedClaudeAppMapping {
                family: ClaudeAppModelMapping {
                    sonnet_model: Some("gpt-5.4".to_string()),
                    default_model: Some("gpt-5.4".to_string()),
                    ..Default::default()
                },
                exact: HashMap::new(),
            }),
        );
        assert_eq!(
            rewritten,
            vec![
                OsString::from("--output-format"),
                OsString::from("stream-json"),
                OsString::from("--model"),
                OsString::from("gpt-5.4"),
            ]
        );
    }

    #[test]
    fn rewrite_model_args_appends_when_missing() {
        let args = vec![
            OsString::from("--output-format"),
            OsString::from("stream-json"),
        ];

        let rewritten = rewrite_model_args(
            &args,
            Some(&ResolvedClaudeAppMapping {
                family: ClaudeAppModelMapping {
                    default_model: Some("gpt-5.4".to_string()),
                    sonnet_model: Some("gpt-5.4".to_string()),
                    ..Default::default()
                },
                exact: HashMap::new(),
            }),
        );
        assert_eq!(
            rewritten,
            vec![
                OsString::from("--output-format"),
                OsString::from("stream-json"),
                OsString::from("--model"),
                OsString::from("gpt-5.4"),
            ]
        );
    }

    #[test]
    fn target_model_from_mapping_uses_family_specific_override() {
        let mapping = ResolvedClaudeAppMapping {
            family: ClaudeAppModelMapping {
                default_model: Some("gpt-5.4".to_string()),
                haiku_model: Some("gpt-4.1-mini".to_string()),
                sonnet_model: Some("gpt-5.4".to_string()),
                opus_model: Some("o3".to_string()),
                thinking_model: Some("o4-mini".to_string()),
            },
            exact: HashMap::new(),
        };

        assert_eq!(
            ClaudeAppBridgeService::target_model_from_mapping(
                &mapping,
                Some("claude-haiku-4-5"),
                false
            )
            .as_deref(),
            Some("gpt-4.1-mini")
        );
        assert_eq!(
            ClaudeAppBridgeService::target_model_from_mapping(
                &mapping,
                Some("claude-opus-4-6"),
                false
            )
            .as_deref(),
            Some("o3")
        );
        assert_eq!(
            ClaudeAppBridgeService::target_model_from_mapping(
                &mapping,
                Some("claude-sonnet-4-6"),
                true
            )
            .as_deref(),
            Some("o4-mini")
        );
    }

    #[test]
    fn target_model_from_mapping_preserves_already_mapped_upstream_model() {
        let mapping = ResolvedClaudeAppMapping {
            family: ClaudeAppModelMapping {
                default_model: Some("gpt-5.2".to_string()),
                haiku_model: Some("gpt-5.4-mini".to_string()),
                sonnet_model: Some("gpt-5.2".to_string()),
                opus_model: Some("gpt-5.4".to_string()),
                thinking_model: Some("o4-mini".to_string()),
            },
            exact: HashMap::new(),
        };

        assert_eq!(
            ClaudeAppBridgeService::target_model_from_mapping(
                &mapping,
                Some("gpt-5.4-mini"),
                false
            )
            .as_deref(),
            Some("gpt-5.4-mini")
        );
        assert_eq!(
            ClaudeAppBridgeService::target_model_from_mapping(&mapping, Some("gpt-5.4"), false)
                .as_deref(),
            Some("gpt-5.4")
        );
    }

    #[test]
    fn target_model_from_mapping_prefers_exact_mapping() {
        let mapping = ResolvedClaudeAppMapping {
            family: ClaudeAppModelMapping {
                default_model: Some("gpt-5.4".to_string()),
                sonnet_model: Some("gpt-5.4".to_string()),
                ..Default::default()
            },
            exact: HashMap::from([("claude-sonnet-5-0".to_string(), "o3".to_string())]),
        };

        assert_eq!(
            ClaudeAppBridgeService::target_model_from_mapping(
                &mapping,
                Some("claude-sonnet-5-0"),
                false,
            )
            .as_deref(),
            Some("o3")
        );
    }

    #[test]
    fn resolve_model_mapping_collects_exact_mappings() {
        let mut provider = Provider::with_id(
            "codex-auto".to_string(),
            "Codex Auto".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_MODEL": "gpt-5.4",
                    "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5.4"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            claude_app_exact_model_mappings: vec![ClaudeAppExactModelMappingEntry {
                source_model: "claude-opus-5-0".to_string(),
                target_model: "o3".to_string(),
            }],
            ..Default::default()
        });

        let mapping = ClaudeAppBridgeService::resolve_model_mapping(&provider).unwrap();
        assert_eq!(mapping.exact.get("claude-opus-5-0"), Some(&"o3".to_string()));
    }
}
