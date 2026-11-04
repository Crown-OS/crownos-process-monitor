//! The settings dialog and the `process-monitor` config section behind it.

mod alerts;
mod choice;
mod dialog;
mod insights;
mod insights_section;
mod model;
mod persist;
mod store;

pub use alerts::{
    AlertSettings, CpuAlertLevel, GpuAlertLevel, GrowthAlertLevel, HeatAlertLevel,
    NotificationMode, SustainMinutes,
};
pub use choice::Choice;
pub use dialog::settings_dialog;
pub use insights::{
    INSIGHTS_SECTION, InsightSettings, LimitValue, QuietTarget, SavedApp, SavedLimit, SavedQuiet,
};
pub use model::{
    ChartWindow, CpuScale, HistoryRetention, MonitorSettings, SamplingInterval, Spacing,
};
pub use persist::{SettingsWriter, load_insights};
pub use store::Settings;

/// Stored as `~/.config/crownos/process-monitor.ron`.
pub const SETTINGS_SECTION: &str = "process-monitor";
