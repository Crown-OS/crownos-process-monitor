use crownui::AppConfig;
use crownui::kit::{INTER_FAMILY, inter_font};

const APP_ID: &str = "crownos-process-monitor";
const TITLE: &str = "Process Monitor";
const INITIAL_SIZE: (u32, u32) = (1180, 780);
const MIN_SIZE: (u32, u32) = (820, 560);

/// The window the monitor opens: title, app id, sizes and the Inter face.
pub fn app_config() -> AppConfig {
    AppConfig::new(TITLE)
        .app_id(APP_ID)
        .size(INITIAL_SIZE.0, INITIAL_SIZE.1)
        .min_size(MIN_SIZE.0, MIN_SIZE.1)
        .font(inter_font().to_vec(), Some(INTER_FAMILY))
}
