use crate::proxy::providers::codex_auto_auth::CodexAutoAuthManager;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct CodexAutoAuthState(pub Arc<RwLock<CodexAutoAuthManager>>);
