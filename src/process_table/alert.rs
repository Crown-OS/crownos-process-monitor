use std::sync::Arc;

use monitor_core::{AppKey, ProcessKey};

/// Which alert menu row was picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertWish {
    /// "Alert when…": open a rule editor preset to this app or process.
    AlertWhen,
    /// "Don't alert about…": quiet alerts about it.
    DontAlert,
}

/// Raised by a table's context menu through `AppState::alert_request`; the alerts owner turns
/// it into a rule or a quiet scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertRequest {
    pub wish: AlertWish,
    pub app: Option<AppKey>,
    pub process: Option<ProcessKey>,
    pub name: Arc<str>,
}
