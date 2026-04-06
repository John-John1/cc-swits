use crate::proxy::providers::gemini_auto_auth::GeminiAutoAuthManager;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct GeminiAutoAuthState(pub Arc<RwLock<GeminiAutoAuthManager>>);
