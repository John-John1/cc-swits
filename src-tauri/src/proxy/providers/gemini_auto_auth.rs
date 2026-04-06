//! Managed Google Gemini OAuth accounts for CC Switch.
//!
//! This mirrors Codex Auto's managed-auth flow, but stores Google OAuth
//! credentials for Gemini Code Assist style upstreams and keeps
//! `~/.gemini/oauth_creds.json` in sync with the selected default account.

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
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use uuid::Uuid;

const GOOGLE_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
const GOOGLE_CLIENT_SECRET: &str = "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";
const GOOGLE_AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const GOOGLE_REDIRECT_URI: &str = "http://127.0.0.1:1456/oauth2callback";
const GOOGLE_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile";
const LOGIN_SESSION_TTL_SECS: i64 = 600;
const LOGIN_POLL_INTERVAL_SECS: u64 = 2;
const TOKEN_REFRESH_SKEW_SECS: i64 = 60;

pub const GEMINI_AUTO_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";

#[derive(Debug, thiserror::Error)]
pub enum GeminiAutoAuthError {
    #[error("登录会话未启动")]
    LoginNotStarted,
    #[error("等待 Google 授权完成")]
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
    #[error("缺少 refresh_token，无法刷新 Gemini Auto 凭据")]
    MissingRefreshToken,
    #[error("本地回调服务错误: {0}")]
    CallbackServerError(String),
}

impl From<reqwest::Error> for GeminiAutoAuthError {
    fn from(err: reqwest::Error) -> Self {
        GeminiAutoAuthError::NetworkError(err.to_string())
    }
}

impl From<std::io::Error> for GeminiAutoAuthError {
    fn from(err: std::io::Error) -> Self {
        GeminiAutoAuthError::IoError(err.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiAutoAccount {
    pub id: String,
    pub login: String,
    pub avatar_url: Option<String>,
    pub authenticated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiAutoAuthStatus {
    pub accounts: Vec<GeminiAutoAccount>,
    pub default_account_id: Option<String>,
    pub migration_error: Option<String>,
    pub authenticated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiAutoLoginSession {
    pub session_id: String,
    pub authorization_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiAutoTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expiry_date: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeminiAutoAccountData {
    tokens: GeminiAutoTokens,
    login: String,
    avatar_url: Option<String>,
    authenticated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct GeminiAutoAuthStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: HashMap<String, GeminiAutoAccountData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    scope: Option<String>,
    token_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleUserInfoResponse {
    id: Option<String>,
    email: Option<String>,
    name: Option<String>,
    picture: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiOAuthCredsFile {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expiry_date: Option<i64>,
    token_type: Option<String>,
    scope: Option<String>,
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

struct ParsedIdentity {
    account_id: String,
    login: String,
    avatar_url: Option<String>,
}

pub struct GeminiAutoAuthManager {
    accounts: Arc<RwLock<HashMap<String, GeminiAutoAccountData>>>,
    default_account_id: Arc<RwLock<Option<String>>>,
    pending_login: Arc<Mutex<Option<PendingLogin>>>,
    migration_error: Arc<RwLock<Option<String>>>,
    http_client: Client,
    storage_path: PathBuf,
}

impl GeminiAutoAuthManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let storage_path = data_dir.join("gemini_auto_auth.json");
        let manager = Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            default_account_id: Arc::new(RwLock::new(None)),
            pending_login: Arc::new(Mutex::new(None)),
            migration_error: Arc::new(RwLock::new(None)),
            http_client: Client::new(),
            storage_path,
        };

        if let Err(err) = manager.load_from_disk_sync() {
            log::warn!("[GeminiAutoAuth] Failed to load store: {err}");
            if let Ok(mut migration_error) = manager.migration_error.try_write() {
                *migration_error = Some(err.to_string());
            }
        }

        if let Err(err) = manager.import_live_auth_sync() {
            log::warn!("[GeminiAutoAuth] Failed to import oauth_creds.json: {err}");
            if let Ok(mut migration_error) = manager.migration_error.try_write() {
                *migration_error = Some(err.to_string());
            }
        }

        manager
    }

    pub async fn list_accounts(&self) -> Vec<GeminiAutoAccount> {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        Self::sorted_accounts(&accounts, default_account_id.as_deref())
    }

    pub async fn get_status(&self) -> GeminiAutoAuthStatus {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;
        let migration_error = self.migration_error.read().await.clone();
        let account_list = Self::sorted_accounts(&accounts, default_account_id.as_deref());

        GeminiAutoAuthStatus {
            authenticated: !account_list.is_empty(),
            accounts: account_list,
            default_account_id,
            migration_error,
        }
    }

    pub async fn get_valid_token(&self) -> Result<String, GeminiAutoAuthError> {
        let default_account_id = self
            .resolve_default_account_id()
            .await
            .ok_or(GeminiAutoAuthError::LoginNotStarted)?;
        self.get_valid_token_for_account(&default_account_id).await
    }

    pub async fn get_valid_token_for_account(
        &self,
        account_id: &str,
    ) -> Result<String, GeminiAutoAuthError> {
        self.ensure_fresh_token(account_id).await?;
        let accounts = self.accounts.read().await;
        let account = accounts
            .get(account_id)
            .ok_or_else(|| GeminiAutoAuthError::AccountNotFound(account_id.to_string()))?;
        Ok(account.tokens.access_token.clone())
    }

    pub async fn start_login(&self) -> Result<GeminiAutoLoginSession, GeminiAutoAuthError> {
        self.clear_pending_login().await;

        let verifier = Self::create_pkce_verifier();
        let challenge = Self::create_pkce_challenge(&verifier);
        let state = Uuid::new_v4().simple().to_string();
        let session_id = Uuid::new_v4().to_string();
        let expires_at = Utc::now().timestamp() + LOGIN_SESSION_TTL_SECS;
        let authorization_url = Self::build_authorization_url(&state, &challenge)?;

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 1456))
            .await
            .map_err(|err| {
                GeminiAutoAuthError::CallbackServerError(format!(
                    "无法绑定 http://127.0.0.1:1456: {err}"
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

        Ok(GeminiAutoLoginSession {
            session_id,
            authorization_url,
            expires_in: LOGIN_SESSION_TTL_SECS as u64,
            interval: LOGIN_POLL_INTERVAL_SECS,
        })
    }

    pub async fn poll_for_token(
        &self,
        session_id: &str,
    ) -> Result<Option<GeminiAutoAccount>, GeminiAutoAuthError> {
        let pending = {
            let mut pending = self.pending_login.lock().await;
            let Some(current) = pending.as_mut() else {
                return Err(GeminiAutoAuthError::LoginNotStarted);
            };

            if current.session_id != session_id {
                return Err(GeminiAutoAuthError::LoginNotStarted);
            }

            if Utc::now().timestamp() > current.expires_at {
                let stale = pending.take();
                drop(pending);
                if let Some(mut stale) = stale {
                    if let Some(shutdown) = stale.shutdown.take() {
                        let _ = shutdown.send(());
                    }
                }
                return Err(GeminiAutoAuthError::ExpiredToken);
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
                    return Err(GeminiAutoAuthError::CallbackServerError(error));
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
                    return Err(GeminiAutoAuthError::CallbackServerError(
                        "授权回调服务已关闭".to_string(),
                    ));
                }
            }
        };

        let (Some(code), Some(mut pending_login)) = pending else {
            return Err(GeminiAutoAuthError::AuthorizationPending);
        };

        if let Some(shutdown) = pending_login.shutdown.take() {
            let _ = shutdown.send(());
        }

        let token_response = self
            .exchange_authorization_code(&code, &pending_login.verifier)
            .await?;
        let tokens = Self::build_tokens(token_response)?;
        let identity = self.fetch_identity(&tokens.access_token).await?;
        let account = self.upsert_account(tokens, identity, true).await?;

        Ok(Some(account))
    }

    pub async fn set_default_account(
        &self,
        account_id: &str,
    ) -> Result<(), GeminiAutoAuthError> {
        {
            let accounts = self.accounts.read().await;
            if !accounts.contains_key(account_id) {
                return Err(GeminiAutoAuthError::AccountNotFound(account_id.to_string()));
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

    pub async fn remove_account(&self, account_id: &str) -> Result<(), GeminiAutoAuthError> {
        {
            let mut accounts = self.accounts.write().await;
            if accounts.remove(account_id).is_none() {
                return Err(GeminiAutoAuthError::AccountNotFound(account_id.to_string()));
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

    pub async fn clear_auth(&self) -> Result<(), GeminiAutoAuthError> {
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

        let creds_path = Self::get_live_auth_path();
        if creds_path.exists() {
            fs::remove_file(creds_path)?;
        }

        crate::gemini_config::write_google_oauth_settings()
            .map_err(|err| GeminiAutoAuthError::IoError(err.to_string()))?;

        Ok(())
    }

    async fn ensure_fresh_token(&self, account_id: &str) -> Result<(), GeminiAutoAuthError> {
        let maybe_refresh_token = {
            let accounts = self.accounts.read().await;
            let account = accounts
                .get(account_id)
                .ok_or_else(|| GeminiAutoAuthError::AccountNotFound(account_id.to_string()))?;
            if Self::is_token_stale(account.tokens.expiry_date) {
                Some(account.tokens.refresh_token.clone())
            } else {
                None
            }
        };

        let Some(refresh_token) = maybe_refresh_token else {
            return Ok(());
        };

        let refreshed = self.refresh_access_token(&refresh_token).await?;
        {
            let mut accounts = self.accounts.write().await;
            let account = accounts
                .get_mut(account_id)
                .ok_or_else(|| GeminiAutoAuthError::AccountNotFound(account_id.to_string()))?;
            if let Some(access_token) = refreshed.access_token.filter(|value| !value.trim().is_empty()) {
                account.tokens.access_token = access_token;
            }
            account.tokens.expiry_date =
                (Utc::now().timestamp() + refreshed.expires_in.unwrap_or(3600) as i64) * 1000;
            account.tokens.scope = refreshed.scope;
            account.tokens.token_type = refreshed.token_type;
            if let Some(refresh_token) = refreshed.refresh_token.filter(|value| !value.trim().is_empty()) {
                account.tokens.refresh_token = refresh_token;
            }
        }

        self.save_to_disk().await?;
        if self.resolve_default_account_id().await.as_deref() == Some(account_id) {
            self.sync_live_auth_to_default().await?;
        }
        Ok(())
    }

    async fn upsert_account(
        &self,
        tokens: GeminiAutoTokens,
        identity: ParsedIdentity,
        make_default: bool,
    ) -> Result<GeminiAutoAccount, GeminiAutoAuthError> {
        let now = Utc::now().timestamp();
        let account = GeminiAutoAccount {
            id: identity.account_id.clone(),
            login: identity.login.clone(),
            avatar_url: identity.avatar_url.clone(),
            authenticated_at: now,
        };

        {
            let mut accounts = self.accounts.write().await;
            accounts.insert(
                identity.account_id.clone(),
                GeminiAutoAccountData {
                    tokens,
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

    async fn sync_live_auth_to_default(&self) -> Result<(), GeminiAutoAuthError> {
        let default_account_id = self.resolve_default_account_id().await;
        let creds_path = Self::get_live_auth_path();

        let Some(default_account_id) = default_account_id else {
            if creds_path.exists() {
                fs::remove_file(&creds_path)?;
            }
            crate::gemini_config::write_google_oauth_settings()
                .map_err(|err| GeminiAutoAuthError::IoError(err.to_string()))?;
            return Ok(());
        };

        let tokens = {
            let accounts = self.accounts.read().await;
            let account = accounts
                .get(&default_account_id)
                .ok_or_else(|| GeminiAutoAuthError::AccountNotFound(default_account_id.clone()))?;
            account.tokens.clone()
        };

        let payload = serde_json::json!({
            "access_token": tokens.access_token,
            "refresh_token": tokens.refresh_token,
            "expiry_date": tokens.expiry_date,
            "token_type": tokens.token_type,
            "scope": tokens.scope,
        });

        crate::config::write_json_file(&creds_path, &payload)
            .map_err(|err| GeminiAutoAuthError::IoError(err.to_string()))?;
        crate::gemini_config::write_google_oauth_settings()
            .map_err(|err| GeminiAutoAuthError::IoError(err.to_string()))?;
        Ok(())
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

    async fn save_to_disk(&self) -> Result<(), GeminiAutoAuthError> {
        let accounts = self.accounts.read().await.clone();
        let default_account_id = self.resolve_default_account_id().await;

        let store = GeminiAutoAuthStore {
            version: 1,
            accounts,
            default_account_id,
        };

        let content = serde_json::to_string_pretty(&store)
            .map_err(|err| GeminiAutoAuthError::ParseError(err.to_string()))?;
        self.write_store_atomic(&content)
    }

    fn load_from_disk_sync(&self) -> Result<(), GeminiAutoAuthError> {
        if !self.storage_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&self.storage_path)?;
        let store: GeminiAutoAuthStore = serde_json::from_str(&content)
            .map_err(|err| GeminiAutoAuthError::ParseError(err.to_string()))?;

        if let Ok(mut accounts) = self.accounts.try_write() {
            *accounts = store.accounts;
        }
        if let Ok(mut default_account_id) = self.default_account_id.try_write() {
            *default_account_id = store.default_account_id;
        }

        Ok(())
    }

    fn import_live_auth_sync(&self) -> Result<(), GeminiAutoAuthError> {
        let auth_path = Self::get_live_auth_path();
        if !auth_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&auth_path)?;
        let live_auth: GeminiOAuthCredsFile = serde_json::from_str(&content)
            .map_err(|err| GeminiAutoAuthError::ParseError(err.to_string()))?;

        let access_token = live_auth
            .access_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| GeminiAutoAuthError::ParseError("缺少 access_token".to_string()))?;
        let refresh_token = live_auth
            .refresh_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| GeminiAutoAuthError::ParseError("缺少 refresh_token".to_string()))?;
        let expiry_date = live_auth
            .expiry_date
            .unwrap_or_else(|| (Utc::now().timestamp() + 3600) * 1000);

        let identity = Self::resolve_identity_from_jwt(&access_token).unwrap_or_else(|| {
            let short = access_token.chars().rev().take(6).collect::<String>();
            ParsedIdentity {
                account_id: format!("gemini-{}", short.chars().rev().collect::<String>()),
                login: "Gemini Auto".to_string(),
                avatar_url: None,
            }
        });

        if let Ok(mut accounts) = self.accounts.try_write() {
            accounts.insert(
                identity.account_id.clone(),
                GeminiAutoAccountData {
                    tokens: GeminiAutoTokens {
                        access_token,
                        refresh_token,
                        expiry_date,
                        token_type: live_auth.token_type,
                        scope: live_auth.scope,
                    },
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
        let store = GeminiAutoAuthStore {
            version: 1,
            accounts,
            default_account_id,
        };
        let content = serde_json::to_string_pretty(&store)
            .map_err(|err| GeminiAutoAuthError::ParseError(err.to_string()))?;
        self.write_store_atomic(&content)?;

        Ok(())
    }

    fn write_store_atomic(&self, content: &str) -> Result<(), GeminiAutoAuthError> {
        crate::config::atomic_write(&self.storage_path, content.as_bytes())
            .map_err(|err| GeminiAutoAuthError::IoError(err.to_string()))
    }

    async fn exchange_authorization_code(
        &self,
        code: &str,
        verifier: &str,
    ) -> Result<GoogleTokenResponse, GeminiAutoAuthError> {
        let response = self
            .http_client
            .post(GOOGLE_TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", GOOGLE_CLIENT_ID),
                ("client_secret", GOOGLE_CLIENT_SECRET),
                ("code", code),
                ("code_verifier", verifier),
                ("redirect_uri", GOOGLE_REDIRECT_URI),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(GeminiAutoAuthError::NetworkError(format!(
                "Google token exchange failed: {status} - {text}"
            )));
        }

        response
            .json::<GoogleTokenResponse>()
            .await
            .map_err(|err| GeminiAutoAuthError::ParseError(err.to_string()))
    }

    async fn refresh_access_token(
        &self,
        refresh_token: &str,
    ) -> Result<GoogleTokenResponse, GeminiAutoAuthError> {
        let response = self
            .http_client
            .post(GOOGLE_TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", GOOGLE_CLIENT_ID),
                ("client_secret", GOOGLE_CLIENT_SECRET),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(GeminiAutoAuthError::NetworkError(format!(
                "Google token refresh failed: {status} - {text}"
            )));
        }

        response
            .json::<GoogleTokenResponse>()
            .await
            .map_err(|err| GeminiAutoAuthError::ParseError(err.to_string()))
    }

    fn build_tokens(
        token_response: GoogleTokenResponse,
    ) -> Result<GeminiAutoTokens, GeminiAutoAuthError> {
        let access_token = token_response
            .access_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| GeminiAutoAuthError::ParseError("缺少 access_token".to_string()))?;
        let refresh_token = token_response
            .refresh_token
            .filter(|value| !value.trim().is_empty())
            .ok_or(GeminiAutoAuthError::MissingRefreshToken)?;
        let expires_in = token_response.expires_in.unwrap_or(3600) as i64;

        Ok(GeminiAutoTokens {
            access_token,
            refresh_token,
            expiry_date: (Utc::now().timestamp() + expires_in) * 1000,
            scope: token_response.scope,
            token_type: token_response.token_type,
        })
    }

    async fn fetch_identity(
        &self,
        access_token: &str,
    ) -> Result<ParsedIdentity, GeminiAutoAuthError> {
        let response = self
            .http_client
            .get(GOOGLE_USERINFO_URL)
            .bearer_auth(access_token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(GeminiAutoAuthError::NetworkError(format!(
                "Google userinfo failed: {status} - {text}"
            )));
        }

        let userinfo = response
            .json::<GoogleUserInfoResponse>()
            .await
            .map_err(|err| GeminiAutoAuthError::ParseError(err.to_string()))?;

        let account_id = userinfo
            .id
            .filter(|value| !value.trim().is_empty())
            .or_else(|| userinfo.email.clone())
            .ok_or_else(|| GeminiAutoAuthError::ParseError("无法解析 Google 用户 ID".to_string()))?;

        let login = userinfo
            .email
            .clone()
            .or(userinfo.name)
            .unwrap_or_else(|| format!("Google {}", Self::short_account_id(&account_id)));

        Ok(ParsedIdentity {
            account_id,
            login,
            avatar_url: userinfo.picture,
        })
    }

    fn resolve_identity_from_jwt(access_token: &str) -> Option<ParsedIdentity> {
        let payload = Self::decode_jwt_payload(access_token)?;
        let email = payload
            .get("email")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let name = payload
            .get("name")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let picture = payload
            .get("picture")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let sub = payload
            .get("sub")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| email.clone())?;

        Some(ParsedIdentity {
            account_id: sub.clone(),
            login: email
                .or(name)
                .unwrap_or_else(|| format!("Google {}", Self::short_account_id(&sub))),
            avatar_url: picture,
        })
    }

    fn decode_jwt_payload(token: &str) -> Option<serde_json::Value> {
        let payload = token.split('.').nth(1)?;
        let decoded = URL_SAFE_NO_PAD
            .decode(payload)
            .ok()
            .or_else(|| URL_SAFE.decode(payload).ok())?;
        serde_json::from_slice(&decoded).ok()
    }

    fn get_live_auth_path() -> PathBuf {
        crate::gemini_config::get_gemini_dir().join("oauth_creds.json")
    }

    fn is_token_stale(expiry_date_ms: i64) -> bool {
        let now_ms = Utc::now().timestamp_millis();
        expiry_date_ms <= now_ms + (TOKEN_REFRESH_SKEW_SECS * 1000)
    }

    fn short_account_id(account_id: &str) -> &str {
        account_id.get(..8).unwrap_or(account_id)
    }

    fn create_pkce_verifier() -> String {
        format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
    }

    fn create_pkce_challenge(verifier: &str) -> String {
        let digest = Sha256::digest(verifier.as_bytes());
        URL_SAFE_NO_PAD.encode(digest)
    }

    fn build_authorization_url(
        state: &str,
        challenge: &str,
    ) -> Result<String, GeminiAutoAuthError> {
        let mut url = url::Url::parse(GOOGLE_AUTHORIZE_URL)
            .map_err(|err| GeminiAutoAuthError::ParseError(err.to_string()))?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", GOOGLE_CLIENT_ID)
            .append_pair("redirect_uri", GOOGLE_REDIRECT_URI)
            .append_pair("scope", GOOGLE_SCOPE)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", state)
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent")
            .append_pair("include_granted_scopes", "true");
        Ok(url.to_string())
    }

    fn sorted_accounts(
        accounts: &HashMap<String, GeminiAutoAccountData>,
        default_account_id: Option<&str>,
    ) -> Vec<GeminiAutoAccount> {
        let mut account_list: Vec<GeminiAutoAccount> = accounts
            .iter()
            .map(|(account_id, data)| GeminiAutoAccount {
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
        accounts: &HashMap<String, GeminiAutoAccountData>,
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
        .route("/oauth2callback", get(handle_callback))
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
        return Html(error_page("Google 授权未完成，请返回 CC Switch 重试。"));
    }

    if params.get("state").map(String::as_str) != Some(state.expected_state.as_str()) {
        return Html(error_page("授权状态校验失败，请返回 CC Switch 重试。"));
    }

    let Some(code) = params.get("code") else {
        let _ = state
            .sender
            .send(LoginEvent::Error("missing authorization code".to_string()));
        return Html(error_page("缺少授权码，请返回 CC Switch 重试。"));
    };

    let _ = state.sender.send(LoginEvent::Code(code.clone()));
    Html(success_page("Google 授权已完成，可以返回 CC Switch。"))
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
