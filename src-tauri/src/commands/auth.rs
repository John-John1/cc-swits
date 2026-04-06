use tauri::State;

use crate::commands::codex_auto::CodexAutoAuthState;
use crate::commands::copilot::CopilotAuthState;
use crate::commands::gemini_auto::GeminiAutoAuthState;
use crate::proxy::providers::codex_auto_auth::{CodexAutoAccount, CodexAutoLoginSession};
use crate::proxy::providers::copilot_auth::{GitHubAccount, GitHubDeviceCodeResponse};
use crate::proxy::providers::gemini_auto_auth::{GeminiAutoAccount, GeminiAutoLoginSession};

const AUTH_PROVIDER_CODEX_AUTO: &str = "codex_auto";
const AUTH_PROVIDER_GITHUB_COPILOT: &str = "github_copilot";
const AUTH_PROVIDER_GEMINI_AUTO: &str = "gemini_auto";

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagedAuthAccount {
    pub id: String,
    pub provider: String,
    pub login: String,
    pub avatar_url: Option<String>,
    pub authenticated_at: i64,
    pub is_default: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagedAuthStatus {
    pub provider: String,
    pub authenticated: bool,
    pub default_account_id: Option<String>,
    pub migration_error: Option<String>,
    pub accounts: Vec<ManagedAuthAccount>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManagedAuthDeviceCodeResponse {
    pub provider: String,
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

fn ensure_auth_provider(auth_provider: &str) -> Result<&str, String> {
    match auth_provider {
        AUTH_PROVIDER_CODEX_AUTO => Ok(AUTH_PROVIDER_CODEX_AUTO),
        AUTH_PROVIDER_GITHUB_COPILOT => Ok(AUTH_PROVIDER_GITHUB_COPILOT),
        AUTH_PROVIDER_GEMINI_AUTO => Ok(AUTH_PROVIDER_GEMINI_AUTO),
        _ => Err(format!("Unsupported auth provider: {auth_provider}")),
    }
}

fn map_account(
    provider: &str,
    account: GitHubAccount,
    default_account_id: Option<&str>,
) -> ManagedAuthAccount {
    ManagedAuthAccount {
        is_default: default_account_id == Some(account.id.as_str()),
        id: account.id,
        provider: provider.to_string(),
        login: account.login,
        avatar_url: account.avatar_url,
        authenticated_at: account.authenticated_at,
    }
}

fn map_device_code_response(
    provider: &str,
    response: GitHubDeviceCodeResponse,
) -> ManagedAuthDeviceCodeResponse {
    ManagedAuthDeviceCodeResponse {
        provider: provider.to_string(),
        device_code: response.device_code,
        user_code: response.user_code,
        verification_uri: response.verification_uri,
        expires_in: response.expires_in,
        interval: response.interval,
    }
}

fn map_codex_account(
    provider: &str,
    account: CodexAutoAccount,
    default_account_id: Option<&str>,
) -> ManagedAuthAccount {
    ManagedAuthAccount {
        is_default: default_account_id == Some(account.id.as_str()),
        id: account.id,
        provider: provider.to_string(),
        login: account.login,
        avatar_url: account.avatar_url,
        authenticated_at: account.authenticated_at,
    }
}

fn map_codex_login_session(
    provider: &str,
    session: CodexAutoLoginSession,
) -> ManagedAuthDeviceCodeResponse {
    ManagedAuthDeviceCodeResponse {
        provider: provider.to_string(),
        device_code: session.session_id,
        user_code: String::new(),
        verification_uri: session.authorization_url,
        expires_in: session.expires_in,
        interval: session.interval,
    }
}

fn map_gemini_account(
    provider: &str,
    account: GeminiAutoAccount,
    default_account_id: Option<&str>,
) -> ManagedAuthAccount {
    ManagedAuthAccount {
        is_default: default_account_id == Some(account.id.as_str()),
        id: account.id,
        provider: provider.to_string(),
        login: account.login,
        avatar_url: account.avatar_url,
        authenticated_at: account.authenticated_at,
    }
}

fn map_gemini_login_session(
    provider: &str,
    session: GeminiAutoLoginSession,
) -> ManagedAuthDeviceCodeResponse {
    ManagedAuthDeviceCodeResponse {
        provider: provider.to_string(),
        device_code: session.session_id,
        user_code: String::new(),
        verification_uri: session.authorization_url,
        expires_in: session.expires_in,
        interval: session.interval,
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_start_login(
    auth_provider: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_auto_state: State<'_, CodexAutoAuthState>,
    gemini_auto_state: State<'_, GeminiAutoAuthState>,
) -> Result<ManagedAuthDeviceCodeResponse, String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.read().await;
            let response = auth_manager
                .start_device_flow()
                .await
                .map_err(|e| e.to_string())?;
            Ok(map_device_code_response(auth_provider, response))
        }
        AUTH_PROVIDER_CODEX_AUTO => {
            let auth_manager = codex_auto_state.0.read().await;
            let session = auth_manager.start_login().await.map_err(|e| e.to_string())?;
            Ok(map_codex_login_session(auth_provider, session))
        }
        AUTH_PROVIDER_GEMINI_AUTO => {
            let auth_manager = gemini_auto_state.0.read().await;
            let session = auth_manager.start_login().await.map_err(|e| e.to_string())?;
            Ok(map_gemini_login_session(auth_provider, session))
        }
        _ => unreachable!("validated above"),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_poll_for_account(
    auth_provider: String,
    device_code: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_auto_state: State<'_, CodexAutoAuthState>,
    gemini_auto_state: State<'_, GeminiAutoAuthState>,
) -> Result<Option<ManagedAuthAccount>, String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.write().await;
            match auth_manager.poll_for_token(&device_code).await {
                Ok(account) => {
                    let default_account_id = auth_manager.get_status().await.default_account_id;
                    Ok(account.map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    }))
                }
                Err(crate::proxy::providers::copilot_auth::CopilotAuthError::AuthorizationPending) => {
                    Ok(None)
                }
                Err(e) => Err(e.to_string()),
            }
        }
        AUTH_PROVIDER_CODEX_AUTO => {
            let auth_manager = codex_auto_state.0.read().await;
            match auth_manager.poll_for_token(&device_code).await {
                Ok(account) => {
                    let default_account_id = auth_manager.get_status().await.default_account_id;
                    Ok(account.map(|account| {
                        map_codex_account(auth_provider, account, default_account_id.as_deref())
                    }))
                }
                Err(crate::proxy::providers::codex_auto_auth::CodexAutoAuthError::AuthorizationPending) => {
                    Ok(None)
                }
                Err(e) => Err(e.to_string()),
            }
        }
        AUTH_PROVIDER_GEMINI_AUTO => {
            let auth_manager = gemini_auto_state.0.read().await;
            match auth_manager.poll_for_token(&device_code).await {
                Ok(account) => {
                    let default_account_id = auth_manager.get_status().await.default_account_id;
                    Ok(account.map(|account| {
                        map_gemini_account(auth_provider, account, default_account_id.as_deref())
                    }))
                }
                Err(crate::proxy::providers::gemini_auto_auth::GeminiAutoAuthError::AuthorizationPending) => {
                    Ok(None)
                }
                Err(e) => Err(e.to_string()),
            }
        }
        _ => unreachable!("validated above"),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_list_accounts(
    auth_provider: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_auto_state: State<'_, CodexAutoAuthState>,
    gemini_auto_state: State<'_, GeminiAutoAuthState>,
) -> Result<Vec<ManagedAuthAccount>, String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(status
                .accounts
                .into_iter()
                .map(|account| map_account(auth_provider, account, default_account_id.as_deref()))
                .collect())
        }
        AUTH_PROVIDER_CODEX_AUTO => {
            let auth_manager = codex_auto_state.0.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(status
                .accounts
                .into_iter()
                .map(|account| {
                    map_codex_account(auth_provider, account, default_account_id.as_deref())
                })
                .collect())
        }
        AUTH_PROVIDER_GEMINI_AUTO => {
            let auth_manager = gemini_auto_state.0.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(status
                .accounts
                .into_iter()
                .map(|account| {
                    map_gemini_account(auth_provider, account, default_account_id.as_deref())
                })
                .collect())
        }
        _ => unreachable!("validated above"),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_get_status(
    auth_provider: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_auto_state: State<'_, CodexAutoAuthState>,
    gemini_auto_state: State<'_, GeminiAutoAuthState>,
) -> Result<ManagedAuthStatus, String> {
    let auth_provider = ensure_auth_provider(&auth_provider)?;
    match auth_provider {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(ManagedAuthStatus {
                provider: auth_provider.to_string(),
                authenticated: status.authenticated,
                default_account_id: default_account_id.clone(),
                migration_error: status.migration_error,
                accounts: status
                    .accounts
                    .into_iter()
                    .map(|account| {
                        map_account(auth_provider, account, default_account_id.as_deref())
                    })
                    .collect(),
            })
        }
        AUTH_PROVIDER_CODEX_AUTO => {
            let auth_manager = codex_auto_state.0.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(ManagedAuthStatus {
                provider: auth_provider.to_string(),
                authenticated: status.authenticated,
                default_account_id: default_account_id.clone(),
                migration_error: status.migration_error,
                accounts: status
                    .accounts
                    .into_iter()
                    .map(|account| {
                        map_codex_account(auth_provider, account, default_account_id.as_deref())
                    })
                    .collect(),
            })
        }
        AUTH_PROVIDER_GEMINI_AUTO => {
            let auth_manager = gemini_auto_state.0.read().await;
            let status = auth_manager.get_status().await;
            let default_account_id = status.default_account_id.clone();
            Ok(ManagedAuthStatus {
                provider: auth_provider.to_string(),
                authenticated: status.authenticated,
                default_account_id: default_account_id.clone(),
                migration_error: status.migration_error,
                accounts: status
                    .accounts
                    .into_iter()
                    .map(|account| {
                        map_gemini_account(auth_provider, account, default_account_id.as_deref())
                    })
                    .collect(),
            })
        }
        _ => unreachable!("validated above"),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_remove_account(
    auth_provider: String,
    account_id: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_auto_state: State<'_, CodexAutoAuthState>,
    gemini_auto_state: State<'_, GeminiAutoAuthState>,
) -> Result<(), String> {
    match ensure_auth_provider(&auth_provider)? {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.write().await;
            auth_manager
                .remove_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_CODEX_AUTO => {
            let auth_manager = codex_auto_state.0.read().await;
            auth_manager
                .remove_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_GEMINI_AUTO => {
            let auth_manager = gemini_auto_state.0.read().await;
            auth_manager
                .remove_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        _ => unreachable!("validated above"),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_set_default_account(
    auth_provider: String,
    account_id: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_auto_state: State<'_, CodexAutoAuthState>,
    gemini_auto_state: State<'_, GeminiAutoAuthState>,
) -> Result<(), String> {
    match ensure_auth_provider(&auth_provider)? {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.write().await;
            auth_manager
                .set_default_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_CODEX_AUTO => {
            let auth_manager = codex_auto_state.0.read().await;
            auth_manager
                .set_default_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_GEMINI_AUTO => {
            let auth_manager = gemini_auto_state.0.read().await;
            auth_manager
                .set_default_account(&account_id)
                .await
                .map_err(|e| e.to_string())
        }
        _ => unreachable!("validated above"),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn auth_logout(
    auth_provider: String,
    copilot_state: State<'_, CopilotAuthState>,
    codex_auto_state: State<'_, CodexAutoAuthState>,
    gemini_auto_state: State<'_, GeminiAutoAuthState>,
) -> Result<(), String> {
    match ensure_auth_provider(&auth_provider)? {
        AUTH_PROVIDER_GITHUB_COPILOT => {
            let auth_manager = copilot_state.0.write().await;
            auth_manager.clear_auth().await.map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_CODEX_AUTO => {
            let auth_manager = codex_auto_state.0.read().await;
            auth_manager.clear_auth().await.map_err(|e| e.to_string())
        }
        AUTH_PROVIDER_GEMINI_AUTO => {
            let auth_manager = gemini_auto_state.0.read().await;
            auth_manager.clear_auth().await.map_err(|e| e.to_string())
        }
        _ => unreachable!("validated above"),
    }
}
