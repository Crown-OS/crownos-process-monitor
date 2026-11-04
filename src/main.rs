//! CrownOS Process Monitor: a live system monitor built with CrownUI.

use anyhow::{Context, Result};
use crownos_process_monitor::app;
use crownos_process_monitor::settings::{MonitorSettings, SETTINGS_SECTION, SettingsWriter};
use crownui::AppConfig;
use crownui::kit::{INTER_FAMILY, inter_font};
use monitor_engine::{Engine, EngineConfig};

const APP_ID: &str = "crownos-process-monitor";
const TITLE: &str = "Process Monitor";
const INITIAL_SIZE: (u32, u32) = (1180, 780);
const MIN_SIZE: (u32, u32) = (820, 560);

fn main() -> Result<()> {
    let initial: MonitorSettings = crownconfig::load_section(SETTINGS_SECTION);
    let writer = SettingsWriter::spawn()?;
    let engine = Engine::start(EngineConfig {
        settings: app::engine_settings(initial.sampling, initial.retention),
        notifications: initial.alerts.notifications.engine(),
        ..EngineConfig::default()
    })
    .context("cannot start the monitoring engine")?;
    let config = AppConfig::new(TITLE)
        .app_id(APP_ID)
        .size(INITIAL_SIZE.0, INITIAL_SIZE.1)
        .min_size(MIN_SIZE.0, MIN_SIZE.1)
        .font(inter_font().to_vec(), Some(INTER_FAMILY));
    let result = crownui::run(app::root(initial, writer.clone(), engine.control()), config);
    let stopped = engine.shutdown();
    writer.flush();
    result?;
    Ok(stopped?)
}
