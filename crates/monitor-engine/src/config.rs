use std::path::PathBuf;
use std::time::Duration;

use monitor_alerts::{CustomLimit, Thresholds};
use monitor_core::Interest;
use monitor_history::Retention;

use crate::HelperConnection;

/// What the settings dialog controls.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineSettings {
    /// Sampling interval while the window is visible; `None` keeps 1 s.
    /// It replaces [`Interest::visible_interval`].
    pub visible_interval: Option<Duration>,
    pub retention: Retention,
    pub thresholds: Thresholds,
    pub custom_limits: Vec<CustomLimit>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Notifications {
    /// One `org.freedesktop.Notifications` popup per alert.
    #[default]
    Desktop,
    Off,
}

#[derive(Clone, Debug, Default)]
pub struct EngineConfig {
    pub interest: Interest,
    pub settings: EngineSettings,
    /// Replaces `$XDG_STATE_HOME/crownos-process-monitor/history.sqlite`.
    pub history_path: Option<PathBuf>,
    pub notifications: Notifications,
    /// The optional privileged helper; the system service by default.
    pub helper: HelperConnection,
}
