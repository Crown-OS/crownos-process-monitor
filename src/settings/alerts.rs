use monitor_core::{Bytes, Celsius, Percent};
use monitor_engine::Notifications;
use monitor_engine::alerts::Thresholds;
use monitor_engine::history::LimitThresholds;
use serde::{Deserialize, Serialize};

const MEBIBYTE: u64 = 1 << 20;

/// The alert limits of the Insights section; a few presets each, so the section stays a row
/// of segmented controls and `MonitorSettings` stays `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertSettings {
    pub sustained_cpu: CpuAlertLevel,
    pub sustained_minutes: SustainMinutes,
    pub memory_growth: GrowthAlertLevel,
    pub heat: HeatAlertLevel,
    pub busy_gpu: GpuAlertLevel,
    pub notifications: NotificationMode,
}

impl AlertSettings {
    /// The engine's detection limits; the rest keep the plan's defaults.
    pub fn thresholds(self) -> Thresholds {
        Thresholds {
            sustained_cpu: Percent::new(self.sustained_cpu.percent()),
            sustained_cpu_minutes: self.sustained_minutes.minutes(),
            memory_growth: Bytes::new(self.memory_growth.mebibytes() * MEBIBYTE),
            hot: Celsius::new(self.heat.celsius()),
            busy_gpu: Percent::new(self.busy_gpu.percent()),
            ..Thresholds::default()
        }
    }

    /// What the At a Limit list counts: the plan's 90 % for CPU, the alert limits otherwise.
    pub fn limit_thresholds(self) -> LimitThresholds {
        LimitThresholds {
            gpu_percent: f64::from(self.busy_gpu.percent()),
            celsius: f64::from(self.heat.celsius()),
            ..LimitThresholds::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CpuAlertLevel {
    Fifty,
    #[default]
    Seventy,
    Ninety,
}

impl CpuAlertLevel {
    pub const fn percent(self) -> f32 {
        match self {
            Self::Fifty => 50.0,
            Self::Seventy => 70.0,
            Self::Ninety => 90.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SustainMinutes {
    Five,
    #[default]
    Ten,
    Fifteen,
    Thirty,
}

impl SustainMinutes {
    pub const fn minutes(self) -> u32 {
        match self {
            Self::Five => 5,
            Self::Ten => 10,
            Self::Fifteen => 15,
            Self::Thirty => 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GrowthAlertLevel {
    HalfGigabyte,
    #[default]
    OneGigabyte,
    TwoGigabytes,
    FourGigabytes,
}

impl GrowthAlertLevel {
    pub const fn mebibytes(self) -> u64 {
        match self {
            Self::HalfGigabyte => 512,
            Self::OneGigabyte => 1024,
            Self::TwoGigabytes => 2048,
            Self::FourGigabytes => 4096,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HeatAlertLevel {
    Eighty,
    EightyFive,
    #[default]
    Ninety,
    NinetyFive,
}

impl HeatAlertLevel {
    pub const fn celsius(self) -> f32 {
        match self {
            Self::Eighty => 80.0,
            Self::EightyFive => 85.0,
            Self::Ninety => 90.0,
            Self::NinetyFive => 95.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GpuAlertLevel {
    Eighty,
    #[default]
    Ninety,
    NinetyFive,
}

impl GpuAlertLevel {
    pub const fn percent(self) -> f32 {
        match self {
            Self::Eighty => 80.0,
            Self::Ninety => 90.0,
            Self::NinetyFive => 95.0,
        }
    }
}

/// Desktop popups for new alerts, switched on and off while the app runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NotificationMode {
    #[default]
    Desktop,
    Off,
}

impl NotificationMode {
    pub const fn engine(self) -> Notifications {
        match self {
            Self::Desktop => Notifications::Desktop,
            Self::Off => Notifications::Off,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_engine_defaults() {
        assert_eq!(AlertSettings::default().thresholds(), Thresholds::default());
        assert_eq!(
            AlertSettings::default().limit_thresholds(),
            LimitThresholds::default()
        );
    }

    #[test]
    fn presets_reach_the_engine() {
        let settings = AlertSettings {
            heat: HeatAlertLevel::Eighty,
            memory_growth: GrowthAlertLevel::TwoGigabytes,
            ..AlertSettings::default()
        };
        let thresholds = settings.thresholds();
        assert_eq!(thresholds.hot, Celsius::new(80.0));
        assert_eq!(thresholds.memory_growth, Bytes::new(2 << 30));
        assert_eq!(settings.limit_thresholds().celsius, 80.0);
    }
}
