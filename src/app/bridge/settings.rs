use monitor_engine::EngineSettings;
use monitor_engine::history::Retention;

use crate::settings::{HistoryRetention, SamplingInterval};

/// The engine's share of the settings dialog.
pub fn engine_settings(sampling: SamplingInterval, retention: HistoryRetention) -> EngineSettings {
    EngineSettings {
        visible_interval: Some(sampling.period()),
        retention: match retention {
            HistoryRetention::ThirtyDays => Retention::Days30,
            HistoryRetention::NinetyDays => Retention::Days90,
            HistoryRetention::SixMonths => Retention::Months6,
            HistoryRetention::OneYear => Retention::Year1,
        },
        ..EngineSettings::default()
    }
}
