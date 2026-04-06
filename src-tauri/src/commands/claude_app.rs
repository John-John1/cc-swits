use crate::commands::{CodexAutoAuthState, CopilotAuthState};
use crate::proxy::http_client::get_for_provider;
use crate::proxy::providers::codex_auto_auth::{
    CodexAutoAuthManager, CODEX_AUTO_RESPONSES_URL,
};
use crate::proxy::providers::transform_responses::{
    anthropic_to_responses, augment_codex_auto_responses,
};
use crate::services::model_fetch;
use crate::services::{ClaudeAppBridgeService, ClaudeAppBridgeStatus};
use crate::store::AppState;
use serde_json::json;
use std::collections::HashSet;
use std::time::Duration;

const CODEX_AUTO_MODEL_CANDIDATES: &[&str] = &[
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.2",
    "gpt-5.2-mini",
    "o3",
    "o4-mini",
];

fn normalize_model_name(value: impl Into<String>) -> Option<String> {
    let value = value.into();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn claude_provider_env(provider: &crate::provider::Provider, key: &str) -> Option<String> {
    provider
        .settings_config
        .get("env")
        .and_then(|env| env.get(key))
        .and_then(|value| value.as_str())
        .and_then(|value| normalize_model_name(value.to_string()))
}

fn collect_target_model_candidates(provider: &crate::provider::Provider) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    let mut push = |value: Option<String>| {
        if let Some(value) = value {
            let key = value.to_ascii_lowercase();
            if seen.insert(key) {
                result.push(value);
            }
        }
    };

    push(claude_provider_env(provider, "ANTHROPIC_MODEL"));
    push(claude_provider_env(provider, "ANTHROPIC_REASONING_MODEL"));
    push(claude_provider_env(provider, "ANTHROPIC_DEFAULT_HAIKU_MODEL"));
    push(claude_provider_env(provider, "ANTHROPIC_DEFAULT_SONNET_MODEL"));
    push(claude_provider_env(provider, "ANTHROPIC_DEFAULT_OPUS_MODEL"));

    if let Some(meta) = provider.meta.as_ref() {
        for entry in &meta.claude_app_exact_model_mappings {
            push(normalize_model_name(entry.target_model.clone()));
        }
        for model in &meta.claude_app_fetched_target_models {
            push(normalize_model_name(model.clone()));
        }
    }

    for candidate in CODEX_AUTO_MODEL_CANDIDATES {
        push(normalize_model_name((*candidate).to_string()));
    }

    result
}

async fn probe_codex_auto_model(
    provider: &crate::provider::Provider,
    token: &str,
    model: &str,
) -> bool {
    let anthropic_body = json!({
        "model": model,
        "max_tokens": 16,
        "messages": [{ "role": "user", "content": "ping" }],
        "stream": true
    });

    let Ok(responses_body) = anthropic_to_responses(anthropic_body, Some(&provider.id)) else {
        return false;
    };
    let body = augment_codex_auto_responses(responses_body, Some("You are Codex."));
    let client = get_for_provider(None);
    let mut request_builder = client
        .post(CODEX_AUTO_RESPONSES_URL)
        .timeout(Duration::from_secs(12))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .header("accept-encoding", "identity")
        .header("openai-beta", "responses=experimental")
        .header("originator", "cc_switch")
        .header("user-agent", "cc-switch/3.12.3")
        .header("session_id", format!("claude-app-model-fetch-{}", provider.id))
        .json(&body);

    if let Some(account_id) = CodexAutoAuthManager::extract_account_id_from_token(token) {
        request_builder = request_builder.header("ChatGPT-Account-Id", account_id);
    }

    match request_builder.send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

async fn fetch_codex_auto_target_models(
    provider: &crate::provider::Provider,
    codex_auto_state: tauri::State<'_, CodexAutoAuthState>,
) -> Result<Vec<String>, String> {
    let account_id = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("codex_auto"));
    let auth_manager = codex_auto_state.0.read().await;
    let token = match account_id.as_deref() {
        Some(account_id) => auth_manager
            .get_valid_token_for_account(account_id)
            .await
            .map_err(|e| e.to_string())?,
        None => auth_manager
            .get_valid_token()
            .await
            .map_err(|e| e.to_string())?,
    };
    drop(auth_manager);

    let mut supported = Vec::new();
    for candidate in collect_target_model_candidates(provider)
        .into_iter()
        .take(24)
    {
        if probe_codex_auto_model(provider, &token, &candidate).await {
            supported.push(candidate);
        }
    }

    if supported.is_empty() {
        return Err("No supported Codex Auto target models found from the current lightweight probe.".to_string());
    }

    Ok(supported)
}

async fn fetch_target_models_internal(
    state: &AppState,
    copilot_state: tauri::State<'_, CopilotAuthState>,
    codex_auto_state: tauri::State<'_, CodexAutoAuthState>,
    provider_id: &str,
) -> Result<Vec<String>, String> {
    let provider = state
        .db
        .get_provider_by_id(provider_id, "claude")
        .map_err(|e| format!("Failed to load Claude provider: {e}"))?
        .ok_or_else(|| format!("Provider not found: {provider_id}"))?;

    let provider_type = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        .unwrap_or_default();

    let models = if provider_type == "github_copilot" {
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("github_copilot"));
        let auth_manager = copilot_state.0.read().await;
        let models = match account_id.as_deref() {
            Some(account_id) => auth_manager
                .fetch_models_for_account(account_id)
                .await
                .map_err(|e| e.to_string())?,
            None => auth_manager.fetch_models().await.map_err(|e| e.to_string())?,
        };
        models.into_iter().map(|model| model.id).collect()
    } else if provider_type == "codex_auto" {
        fetch_codex_auto_target_models(&provider, codex_auto_state).await?
    } else {
        let base_url = claude_provider_env(&provider, "ANTHROPIC_BASE_URL")
            .ok_or_else(|| "Base URL is required to fetch target models.".to_string())?;
        let api_key = claude_provider_env(&provider, "ANTHROPIC_AUTH_TOKEN")
            .or_else(|| claude_provider_env(&provider, "ANTHROPIC_API_KEY"))
            .ok_or_else(|| "API key is required to fetch target models.".to_string())?;
        model_fetch::fetch_models(
            &base_url,
            &api_key,
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.is_full_url)
                .unwrap_or(false),
        )
        .await?
        .into_iter()
        .map(|model| model.id)
        .collect()
    };

    ClaudeAppBridgeService::set_fetched_target_models(state.db.as_ref(), provider_id, models)
}

#[tauri::command]
pub async fn get_claude_app_bridge_status(
    state: tauri::State<'_, AppState>,
) -> Result<ClaudeAppBridgeStatus, String> {
    state.claude_app_service.get_status().await
}

#[tauri::command]
pub async fn activate_claude_app_provider(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> Result<ClaudeAppBridgeStatus, String> {
    state
        .claude_app_service
        .activate_provider(&provider_id)
        .await
}

#[tauri::command]
pub async fn stop_claude_app_bridge(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.claude_app_service.stop().await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn fetch_claude_app_target_models(
    state: tauri::State<'_, AppState>,
    copilot_state: tauri::State<'_, CopilotAuthState>,
    codex_auto_state: tauri::State<'_, CodexAutoAuthState>,
    provider_id: String,
) -> Result<Vec<String>, String> {
    fetch_target_models_internal(&state, copilot_state, codex_auto_state, &provider_id).await
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_claude_app_observed_source_models(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<String>, String> {
    ClaudeAppBridgeService::get_observed_source_models(state.db.as_ref(), &provider_id)
}

#[tauri::command(rename_all = "camelCase")]
pub fn clear_claude_app_observed_source_models(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<String>, String> {
    ClaudeAppBridgeService::clear_observed_source_models(state.db.as_ref(), &provider_id)
}

#[tauri::command(rename_all = "camelCase")]
pub fn clear_claude_app_fetched_target_models(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<String>, String> {
    ClaudeAppBridgeService::clear_fetched_target_models(state.db.as_ref(), &provider_id)
}
