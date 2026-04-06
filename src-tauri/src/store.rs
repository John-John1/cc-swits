use crate::database::Database;
use crate::services::{ClaudeAppBridgeService, ProxyService};
use std::sync::Arc;

/// 全局应用状态
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub claude_app_service: ClaudeAppBridgeService,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let proxy_service = ProxyService::new(db.clone());
        let claude_app_service = ClaudeAppBridgeService::new(db.clone(), proxy_service.clone());

        Self {
            db,
            proxy_service,
            claude_app_service,
        }
    }
}
