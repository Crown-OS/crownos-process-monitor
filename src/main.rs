//! CrownOS Process Monitor: a live system monitor built with CrownUI.

use anyhow::{Context, Result};
use crownos_process_monitor::app;
use crownos_process_monitor::settings::{MonitorSettings, SETTINGS_SECTION, SettingsWriter};
use monitor_engine::{Engine, EngineConfig};

fn main() -> Result<()> {
    let initial: MonitorSettings = crownconfig::load_section(SETTINGS_SECTION);
    let writer = SettingsWriter::spawn()?;
    let engine = Engine::start(EngineConfig {
        settings: app::engine_settings(initial.sampling, initial.retention),
        notifications: initial.alerts.notifications.engine(),
        ..EngineConfig::default()
    })
    .context("cannot start the monitoring engine")?;
    let result = crownui::run(
        app::root(initial, writer.clone(), engine.control()),
        app::app_config(),
    );
    let stopped = engine.shutdown();
    writer.flush();
    result?;
    Ok(stopped?)
}
