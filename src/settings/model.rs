use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::alerts::AlertSettings;
use crate::util::format::{RateUnit, TemperatureScale};
use crate::util::theme::Palette;

/// Everything the settings dialog edits, as stored in the config section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorSettings {
    pub temperature: TemperatureScale,
    pub rates: RateUnit,
    pub cpu_scale: CpuScale,
    pub sampling: SamplingInterval,
    pub retention: HistoryRetention,
    pub palette: Palette,
    pub spacing: Spacing,
    pub chart_window: ChartWindow,
    pub alerts: AlertSettings,
}

/// Whether 100 % CPU means one core or the whole machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CpuScale {
    PerCore,
    #[default]
    PerMachine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SamplingInterval {
    HalfSecond,
    #[default]
    OneSecond,
    TwoSeconds,
}

impl SamplingInterval {
    pub const fn period(self) -> Duration {
        match self {
            Self::HalfSecond => Duration::from_millis(500),
            Self::OneSecond => Duration::from_secs(1),
            Self::TwoSeconds => Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HistoryRetention {
    #[default]
    ThirtyDays,
    NinetyDays,
    SixMonths,
    OneYear,
}

impl HistoryRetention {
    pub const fn days(self) -> u32 {
        match self {
            Self::ThirtyDays => 30,
            Self::NinetyDays => 90,
            Self::SixMonths => 182,
            Self::OneYear => 365,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Spacing {
    #[default]
    Comfortable,
    Compact,
}

impl Spacing {
    pub const fn page_padding(self) -> f32 {
        match self {
            Self::Comfortable => 28.0,
            Self::Compact => 16.0,
        }
    }

    pub const fn gap(self) -> f32 {
        match self {
            Self::Comfortable => 20.0,
            Self::Compact => 12.0,
        }
    }

    pub const fn row_height(self) -> f32 {
        match self {
            Self::Comfortable => 34.0,
            Self::Compact => 28.0,
        }
    }
}

/// How much recent history live charts show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChartWindow {
    #[default]
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
}

impl ChartWindow {
    pub const fn seconds(self) -> usize {
        match self {
            Self::FiveMinutes => 5 * 60,
            Self::FifteenMinutes => 15 * 60,
            Self::ThirtyMinutes => 30 * 60,
        }
    }
}
