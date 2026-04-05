//! Managed OpenAI Codex OAuth accounts for CC Switch.
//!
//! This module mirrors the managed-auth experience used by GitHub Copilot, but
//! stores OpenAI Codex OAuth credentials and keeps `~/.codex/auth.json` in sync
//! with the selected default account.

use axum::{
    extract::{Query, State as AxumState},
    response::Html,
    routing::get,
    Router,
};
use base64::{
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
    Engine as _,
};
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use uuid::Uuid;

const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OPENAI_SCOPE: &str = "openid profile email offline_access";
const OPENAI_JWT_AUTH_CLAIM_PATH: &str = "https://api.openai.com/auth";
const OPENAI_JWT_PROFILE_CLAIM_PATH: &str = "https://api.openai.com/profile";
const LOGIN_SESSION_TTL_SECS: i64 = 600;
const LOGIN_POLL_INTERVAL_SECS: u64 = 2;

pub const CODEX_AUTO_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
pub const CODEX_AUTO_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";

#[derive(Debug, thiserror::Error)]
pub enum CodexAutoAuthError {
    #[error("登录会话未启动")]
    LoginNotStarted,

    #[error("等待 OpenAI 授权完成")]
    AuthorizationPending,

    #[error("授权会话已过期")]
    ExpiredToken,

    #[error("网络错误: {0}")]
    NetworkError(String),

    #[error("解析错误: {0}")]
    ParseError(String),

    #[error("IO 错误: {0}")]
    IoError(String),

    #[error("账号不存在: {0}")]
    AccountNotFound(String),

    #[error("本地回调服务错误: {0}")]
    CallbackServerError(String),
}

impl From<reqwest::Error> for CodexAutoAuthError {
    fn from(err: reqwest::Error) -> Self {
        CodexAutoAuthError::NetworkError(err.to_string())
    }
}

impl From<std::io::Error> for CodexAutoAuthError {
    fn from(err: std::io::Error) -> Self {
        CodexAutoAuthError::IoError(err.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexAutoAccount {
    pub id: String,
    pub login: String,
    pub avatar_url: Option<String>,
    pub authenticated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexAutoAuthStatus {
    pub accounts: Vec<CodexAutoAccount>,
    pub default_account_id: Option<String>,
    pub migration_error: Option<String>,
    pub authenticated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexAutoLoginSession {
    pub session_id: String,
    pub authorization_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexAuthTokens {
    #[serde(skip_serializing_if = "Option::is_none")]
    id_token: Option<String>,
    access_token: String,
    refresh_token: String,
    account_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexLiveAuth {
    auth_mode: String,
    #[serde(rename = "OPENAI_API_KEY", default = "null_json_value")]
    openai_api_key: Value,
    tokens: CodexAuthTokens,
    last_refresh: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexAutoAccountData {
    auth: CodexLiveAuth,
    login: String,
    avatar_url: Option<String>,
    authenticated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CodexAutoAuthStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: HashMap<String, CodexAutoAccountData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Debug)]
enum LoginEvent {
    Code(String),
    Error(String),
}

struct PendingLogin {
    session_id: String,
    verifier: String,
    expires_at: i64,
    events: mpsc::UnboundedReceiver<LoginEvent>,
    shutdown: Option<oneshot::Sender<()>>,
}

#[derive(Clone)]
struct CallbackServerState {
    expected_state: String,
    sender: mpsc::UnboundedSender<LoginEvent>,
}

#[derive(Debug)]
struct ParsedIdentity {
    account_id: String,
    login: String,
    avatar_url: Option<String>,
}

fn null_json_value() -> Value {
    Value::Null
}

pub struct CodexAutoAuthManager {
    accounts: Arc<RwLock<HashMap<String, CodexAutoAccountData>>>,
    default_account_id: Arc<RwLock<Option<String>>>,
    pending_login: Arc<Mutex<Option<PendingLogin>>>,
    migration_error: Arc<RwLock<Option<String>>>,
    http_client: Client,
    storage_path: PathBuf,
}

impl CodexAutoAuthManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let storage_path = data_dir.join("codex_auto_auth.json");
        let manager = Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            default_account_id: Arc::new(RwLock::new(None)),
            pending_login: Arc::new(Mutex::new(None)),
            migration_error: Arc::new(RwLock::new(None)),
            http_client: Client::new(),
            storage_path,
        };

        if let Err(err) = manager.load_from_disk_sync() {
            log::warn!("[CodexAutoAuth] Failed to load store: {err}");
            if let Ok(mut migration_error) = manager.migration_error.try_write() {
                *migration_error = Some(err.to_string());
            }
        }

        if let Err(err) = manager.import_live_auth_sync() {
            log::warn!("[CodexAutoAuth] Failed to import live auth.json: {err}");
            if let Ok(mut migration_error) = manager.migration_error.try_write() {
                *migration_error = Some(err.to_string());
            }
        }

        manager
    }

    pub async fn list_accounts(&self) -> Vec<CodexAutoAccount> {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        Self::sorted_accounts(&accounts, default_account_id.as_deref())
    }

    pub async fn get_status(&self) -> CodexAutoAuthStatus {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        let migration_error = self.migration_error.read().await.clone();
        let account_list = Self::sorted_accounts(&accounts, default_account_id.as_deref());

        CodexAutoAuthStatus {
            authenticated: !account_list.is_empty(),
            accounts: account_list,
            default_account_id,
            migration_error,
        }
    }

    pub async fn get_valid_token(&self) -> Result<String, CodexAutoAuthError> {
        let default_account_id = self
            .resolve_default_account_id()
            .await
            .ok_or(CodexAutoAuthError::LoginNotStarted)?;

        self.get_valid_token_for_account(&default_account_id).await
    }

    pub async fn get_valid_token_for_account(
        &self,
        account_id: &str,
    ) -> Result<String, CodexAutoAuthError> {
        let accounts = self.accounts.read().await;
        let account = accounts
            .get(account_id)
            .ok_or_else(|| CodexAutoAuthError::AccountNotFound(account_id.to_string()))?;

        Ok(account.auth.tokens.access_token.clone())
    }

    pub async fn start_login(&self) -> Result<CodexAutoLoginSession, CodexAutoAuthError> {
        self.clear_pending_login().await;

        let verifier = Self::create_pkce_verifier();
        let challenge = Self::create_pkce_challenge(&verifier);
        let state = Uuid::new_v4().simple().to_string();
        let session_id = Uuid::new_v4().to_string();
        let expires_at = Utc::now().timestamp() + LOGIN_SESSION_TTL_SECS;
        let authorization_url = Self::build_authorization_url(&state, &challenge)?;

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 1455))
            .await
            .map_err(|err| {
                CodexAutoAuthError::CallbackServerError(format!(
                    "无法绑定 http://127.0.0.1:1455: {err}"
                ))
            })?;

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let callback_state = CallbackServerState {
            expected_state: state,
            sender: event_tx.clone(),
        };

        tokio::spawn(async move {
            if let Err(err) =
                run_callback_server(listener, callback_state, shutdown_rx, LOGIN_SESSION_TTL_SECS)
                    .await
            {
                let _ = event_tx.send(LoginEvent::Error(err));
            }
        });

        {
            let mut pending = self.pending_login.lock().await;
            *pending = Some(PendingLogin {
                session_id: session_id.clone(),
                verifier,
                expires_at,
                events: event_rx,
                shutdown: Some(shutdown_tx),
            });
        }

        Ok(CodexAutoLoginSession {
            session_id,
            authorization_url,
            expires_in: LOGIN_SESSION_TTL_SECS as u64,
            interval: LOGIN_POLL_INTERVAL_SECS,
        })
    }

    pub async fn poll_for_token(
        &self,
        session_id: &str,
    ) -> Result<Option<CodexAutoAccount>, CodexAutoAuthError> {
        let pending = {
            let mut pending = self.pending_login.lock().await;
            let Some(current) = pending.as_mut() else {
                return Err(CodexAutoAuthError::LoginNotStarted);
            };

            if current.session_id != session_id {
                return Err(CodexAutoAuthError::LoginNotStarted);
            }

            if Utc::now().timestamp() > current.expires_at {
                let stale = pending.take();
                drop(pending);
                if let Some(mut stale) = stale {
                    if let Some(shutdown) = stale.shutdown.take() {
                        let _ = shutdown.send(());
                    }
                }
                return Err(CodexAutoAuthError::ExpiredToken);
            }

            match current.events.try_recv() {
                Ok(LoginEvent::Code(code)) => {
                    let pending_login = pending.take().expect("pending login should exist");
                    (Some(code), Some(pending_login))
                }
                Ok(LoginEvent::Error(error)) => {
                    let failed = pending.take();
                    drop(pending);
                    if let Some(mut failed) = failed {
                        if let Some(shutdown) = failed.shutdown.take() {
                            let _ = shutdown.send(());
                        }
                    }
                    return Err(CodexAutoAuthError::CallbackServerError(error));
                }
                Err(mpsc::error::TryRecvError::Empty) => (None, None),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    let failed = pending.take();
                    drop(pending);
                    if let Some(mut failed) = failed {
                        if let Some(shutdown) = failed.shutdown.take() {
                            let _ = shutdown.send(());
                        }
                    }
                    return Err(CodexAutoAuthError::CallbackServerError(
                        "授权回调服务已关闭".to_string(),
                    ));
                }
            }
        };

        let (Some(code), Some(mut pending_login)) = pending else {
            return Err(CodexAutoAuthError::AuthorizationPending);
        };

        if let Some(shutdown) = pending_login.shutdown.take() {
            let _ = shutdown.send(());
        }

        let token_response = self
            .exchange_authorization_code(&code, &pending_login.verifier)
            .await?;
        let auth = Self::build_live_auth(token_response)?;
        let identity = Self::resolve_identity(&auth)?;
        let account = self.upsert_account(auth, identity, true).await?;

        Ok(Some(account))
    }

    pub async fn set_default_account(
        &self,
        account_id: &str,
    ) -> Result<(), CodexAutoAuthError> {
        {
            let accounts = self.accounts.read().await;
            if !accounts.contains_key(account_id) {
                return Err(CodexAutoAuthError::AccountNotFound(account_id.to_string()));
            }
        }

        {
            let mut default_account_id = self.default_account_id.write().await;
            *default_account_id = Some(account_id.to_string());
        }

        self.save_to_disk().await?;
        self.sync_live_auth_to_default().await?;
        Ok(())
    }

    pub async fn remove_account(
        &self,
        account_id: &str,
    ) -> Result<(), CodexAutoAuthError> {
        {
            let mut accounts = self.accounts.write().await;
            if accounts.remove(account_id).is_none() {
                return Err(CodexAutoAuthError::AccountNotFound(account_id.to_string()));
            }
        }

        {
            let accounts = self.accounts.read().await;
            let mut default_account_id = self.default_account_id.write().await;
            if default_account_id.as_deref() == Some(account_id) {
                *default_account_id = Self::fallback_default_account_id(&accounts);
            }
        }

        self.save_to_disk().await?;
        self.sync_live_auth_to_default().await?;
        Ok(())
    }

    pub async fn clear_auth(&self) -> Result<(), CodexAutoAuthError> {
        {
            let mut accounts = self.accounts.write().await;
            accounts.clear();
        }
        {
            let mut default_account_id = self.default_account_id.write().await;
            *default_account_id = None;
        }

        self.clear_pending_login().await;

        if self.storage_path.exists() {
            fs::remove_file(&self.storage_path)?;
        }

        let auth_path = crate::codex_config::get_codex_auth_path();
        if auth_path.exists() {
            fs::remove_file(auth_path)?;
        }

        Ok(())
    }

    async fn upsert_account(
        &self,
        auth: CodexLiveAuth,
        identity: ParsedIdentity,
        make_default: bool,
    ) -> Result<CodexAutoAccount, CodexAutoAuthError> {
        let now = Utc::now().timestamp();
        let account = CodexAutoAccount {
            id: identity.account_id.clone(),
            login: identity.login.clone(),
            avatar_url: identity.avatar_url.clone(),
            authenticated_at: now,
        };

        {
            let mut accounts = self.accounts.write().await;
            accounts.insert(
                identity.account_id.clone(),
                CodexAutoAccountData {
                    auth,
                    login: identity.login,
                    avatar_url: identity.avatar_url,
                    authenticated_at: now,
                },
            );
        }

        {
            let mut default_account_id = self.default_account_id.write().await;
            if make_default || default_account_id.is_none() {
                *default_account_id = Some(identity.account_id);
            }
        }

        {
            let mut migration_error = self.migration_error.write().await;
            *migration_error = None;
        }

        self.save_to_disk().await?;
        self.sync_live_auth_to_default().await?;

        Ok(account)
    }

    async fn sync_live_auth_to_default(&self) -> Result<(), CodexAutoAuthError> {
        let default_account_id = self.resolve_default_account_id().await;
        let auth_path = crate::codex_config::get_codex_auth_path();

        let Some(default_account_id) = default_account_id else {
            if auth_path.exists() {
                fs::remove_file(auth_path)?;
            }
            return Ok(());
        };

        let auth = {
            let accounts = self.accounts.read().await;
            let account = accounts
                .get(&default_account_id)
                .ok_or_else(|| CodexAutoAuthError::AccountNotFound(default_account_id.clone()))?;
            serde_json::to_value(&account.auth)
                .map_err(|err| CodexAutoAuthError::ParseError(err.to_string()))?
        };

        crate::config::write_json_file(&auth_path, &auth)
            .map_err(|err| CodexAutoAuthError::IoError(err.to_string()))
    }

    async fn resolve_default_account_id(&self) -> Option<String> {
        let accounts = self.accounts.read().await;
        let current_default = self.default_account_id.read().await.clone();
        if let Some(default_account_id) = current_default {
            if accounts.contains_key(&default_account_id) {
                return Some(default_account_id);
            }
        }
        Self::fallback_default_account_id(&accounts)
    }

    async fn clear_pending_login(&self) {
        let pending = {
            let mut pending = self.pending_login.lock().await;
            pending.take()
        };

        if let Some(mut pending) = pending {
            if let Some(shutdown) = pending.shutdown.take() {
                let _ = shutdown.send(());
            }
        }
    }

    async fn save_to_disk(&self) -> Result<(), CodexAutoAuthError> {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;

        let store = CodexAutoAuthStore {
            version: 1,
            accounts,
            default_account_id,
        };

        let content = serde_json::to_string_pretty(&store)
            .map_err(|err| CodexAutoAuthError::ParseError(err.to_string()))?;
        self.write_store_atomic(&content)
    }

    fn load_from_disk_sync(&self) -> Result<(), CodexAutoAuthError> {
        if !self.storage_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&self.storage_path)?;
        let store: CodexAutoAuthStore = serde_json::from_str(&content)
            .map_err(|err| CodexAutoAuthError::ParseError(err.to_string()))?;

        if let Ok(mut accounts) = self.accounts.try_write() {
            *accounts = store.accounts;
        }
        if let Ok(mut default_account_id) = self.default_account_id.try_write() {
            *default_account_id = store.default_account_id;
        }

        Ok(())
    }

    fn import_live_auth_sync(&self) -> Result<(), CodexAutoAuthError> {
        let auth_path = crate::codex_config::get_codex_auth_path();
        if !auth_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&auth_path)?;
        let live_auth: CodexLiveAuth = serde_json::from_str(&content)
            .map_err(|err| CodexAutoAuthError::ParseError(err.to_string()))?;

        if live_auth.auth_mode != "chatgpt" {
            return Ok(());
        }

        let identity = Self::resolve_identity(&live_auth)?;

        if let Ok(mut accounts) = self.accounts.try_write() {
            accounts.insert(
                identity.account_id.clone(),
                CodexAutoAccountData {
                    auth: live_auth,
                    login: identity.login,
                    avatar_url: identity.avatar_url,
                    authenticated_at: Utc::now().timestamp(),
                },
            );
        }

        if let Ok(mut default_account_id) = self.default_account_id.try_write() {
            *default_account_id = Some(identity.account_id);
        }

        let accounts = self
            .accounts
            .try_read()
            .map(|value| value.clone())
            .unwrap_or_default();
        let default_account_id = self
            .default_account_id
            .try_read()
            .map(|value| value.clone())
            .unwrap_or_default();
        let store = CodexAutoAuthStore {
            version: 1,
            accounts,
            default_account_id,
        };
        let content = serde_json::to_string_pretty(&store)
            .map_err(|err| CodexAutoAuthError::ParseError(err.to_string()))?;
        self.write_store_atomic(&content)?;

        Ok(())
    }

    fn write_store_atomic(&self, content: &str) -> Result<(), CodexAutoAuthError> {
        crate::config::atomic_write(&self.storage_path, content.as_bytes())
            .map_err(|err| CodexAutoAuthError::IoError(err.to_string()))
    }

    async fn exchange_authorization_code(
        &self,
        code: &str,
        verifier: &str,
    ) -> Result<OpenAiTokenResponse, CodexAutoAuthError> {
        let response = self
            .http_client
            .post(OPENAI_TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", OPENAI_CLIENT_ID),
                ("code", code),
                ("code_verifier", verifier),
                ("redirect_uri", OPENAI_REDIRECT_URI),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CodexAutoAuthError::NetworkError(format!(
                "OpenAI token exchange failed: {status} - {text}"
            )));
        }

        response
            .json::<OpenAiTokenResponse>()
            .await
            .map_err(|err| CodexAutoAuthError::ParseError(err.to_string()))
    }

    fn build_live_auth(
        token_response: OpenAiTokenResponse,
    ) -> Result<CodexLiveAuth, CodexAutoAuthError> {
        let access_token = token_response
            .access_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| CodexAutoAuthError::ParseError("缺少 access_token".to_string()))?;
        let refresh_token = token_response
            .refresh_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| CodexAutoAuthError::ParseError("缺少 refresh_token".to_string()))?;
        let account_id = Self::extract_account_id_from_token(&access_token).ok_or_else(|| {
            CodexAutoAuthError::ParseError("无法从 access_token 中解析 account_id".to_string())
        })?;

        let _ = token_response.expires_in;

        Ok(CodexLiveAuth {
            auth_mode: "chatgpt".to_string(),
            openai_api_key: Value::Null,
            tokens: CodexAuthTokens {
                id_token: token_response.id_token,
                access_token,
                refresh_token,
                account_id,
            },
            last_refresh: Utc::now().to_rfc3339(),
        })
    }

    fn resolve_identity(auth: &CodexLiveAuth) -> Result<ParsedIdentity, CodexAutoAuthError> {
        let account_id = auth.tokens.account_id.clone();
        let id_token_payload = auth
            .tokens
            .id_token
            .as_deref()
            .and_then(Self::decode_jwt_payload);
        let access_token_payload = Self::decode_jwt_payload(&auth.tokens.access_token);

        let email = id_token_payload
            .as_ref()
            .and_then(|payload| payload.get("email"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                access_token_payload
                    .as_ref()
                    .and_then(|payload| payload.get(OPENAI_JWT_PROFILE_CLAIM_PATH))
                    .and_then(|profile| profile.get("email"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });

        let name = id_token_payload
            .as_ref()
            .and_then(|payload| payload.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let avatar_url = id_token_payload
            .as_ref()
            .and_then(|payload| payload.get("picture"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let login = email
            .or(name)
            .unwrap_or_else(|| format!("Codex {}", Self::short_account_id(&account_id)));

        Ok(ParsedIdentity {
            account_id,
            login,
            avatar_url,
        })
    }

    fn short_account_id(account_id: &str) -> &str {
        account_id.get(..8).unwrap_or(account_id)
    }

    pub fn extract_account_id_from_token(token: &str) -> Option<String> {
        Self::decode_jwt_payload(token)
            .and_then(|payload| payload.get(OPENAI_JWT_AUTH_CLAIM_PATH).cloned())
            .and_then(|auth| auth.get("chatgpt_account_id").cloned())
            .and_then(|value| value.as_str().map(str::to_string))
    }

    fn decode_jwt_payload(token: &str) -> Option<Value> {
        let payload = token.split('.').nth(1)?;
        let decoded = URL_SAFE_NO_PAD
            .decode(payload)
            .ok()
            .or_else(|| URL_SAFE.decode(payload).ok())?;
        serde_json::from_slice(&decoded).ok()
    }

    fn create_pkce_verifier() -> String {
        format!(
            "{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        )
    }

    fn create_pkce_challenge(verifier: &str) -> String {
        let digest = Sha256::digest(verifier.as_bytes());
        URL_SAFE_NO_PAD.encode(digest)
    }

    fn build_authorization_url(
        state: &str,
        challenge: &str,
    ) -> Result<String, CodexAutoAuthError> {
        let mut url = url::Url::parse(OPENAI_AUTHORIZE_URL)
            .map_err(|err| CodexAutoAuthError::ParseError(err.to_string()))?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", OPENAI_CLIENT_ID)
            .append_pair("redirect_uri", OPENAI_REDIRECT_URI)
            .append_pair("scope", OPENAI_SCOPE)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", state)
            .append_pair("id_token_add_organizations", "true")
            .append_pair("codex_cli_simplified_flow", "true")
            .append_pair("originator", "pi");
        Ok(url.to_string())
    }

    fn sorted_accounts(
        accounts: &HashMap<String, CodexAutoAccountData>,
        default_account_id: Option<&str>,
    ) -> Vec<CodexAutoAccount> {
        let mut account_list: Vec<CodexAutoAccount> = accounts
            .iter()
            .map(|(account_id, data)| CodexAutoAccount {
                id: account_id.clone(),
                login: data.login.clone(),
                avatar_url: data.avatar_url.clone(),
                authenticated_at: data.authenticated_at,
            })
            .collect();

        account_list.sort_by(|left, right| {
            let left_is_default = default_account_id == Some(left.id.as_str());
            let right_is_default = default_account_id == Some(right.id.as_str());

            right_is_default
                .cmp(&left_is_default)
                .then_with(|| right.authenticated_at.cmp(&left.authenticated_at))
        });

        account_list
    }

    fn fallback_default_account_id(
        accounts: &HashMap<String, CodexAutoAccountData>,
    ) -> Option<String> {
        accounts
            .iter()
            .max_by_key(|(_, data)| data.authenticated_at)
            .map(|(account_id, _)| account_id.clone())
    }
}

async fn run_callback_server(
    listener: tokio::net::TcpListener,
    state: CallbackServerState,
    shutdown_rx: oneshot::Receiver<()>,
    ttl_secs: i64,
) -> Result<(), String> {
    let app = Router::new()
        .route("/auth/callback", get(handle_callback))
        .with_state(state);

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = async {
                    let _ = shutdown_rx.await;
                } => {}
                _ = tokio::time::sleep(std::time::Duration::from_secs(ttl_secs.max(1) as u64)) => {}
            }
        })
        .await
        .map_err(|err| err.to_string())
}

async fn handle_callback(
    AxumState(state): AxumState<CallbackServerState>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    if let Some(error) = params.get("error") {
        let _ = state.sender.send(LoginEvent::Error(error.clone()));
        return Html(error_page("OpenAI 授权未完成，请返回 CC Switch 重试。"));
    }

    if params.get("state").map(String::as_str) != Some(state.expected_state.as_str()) {
        return Html(error_page("授权状态校验失败，请返回 CC Switch 重试。"));
    }

    let Some(code) = params.get("code") else {
        let _ = state.sender.send(LoginEvent::Error("missing authorization code".to_string()));
        return Html(error_page("缺少授权码，请返回 CC Switch 重试。"));
    };

    let _ = state.sender.send(LoginEvent::Code(code.clone()));
    Html(success_page("OpenAI 授权已完成，可以返回 CC Switch。"))
}

fn success_page(message: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>CC Switch</title></head><body style=\"font-family:system-ui,sans-serif;padding:24px;\"><h2>{message}</h2></body></html>"
    )
}

fn error_page(message: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>CC Switch</title></head><body style=\"font-family:system-ui,sans-serif;padding:24px;\"><h2>{message}</h2></body></html>"
    )
}
