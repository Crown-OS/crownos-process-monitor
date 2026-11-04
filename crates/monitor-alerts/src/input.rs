use std::sync::Arc;

pub use monitor_core::PressureLevel;
use monitor_core::{AppKey, Bytes, BytesPerSecond, Celsius, Percent};

#[derive(Clone, Debug, PartialEq)]
pub struct AppObservation {
    pub key: AppKey,
    pub name: Arc<str>,
    /// Percent of the whole machine.
    pub cpu_percent: Percent,
    pub memory: Bytes,
    pub disk_bytes_per_second: BytesPerSecond,
    pub network_bytes_per_second: BytesPerSecond,
    pub gpu_percent: Percent,
}

/// One sampler tick, borrowed from the engine's snapshot.
#[derive(Clone, Copy, Debug)]
pub struct AlertInput<'a> {
    pub wall_unix_seconds: i64,
    pub elapsed_seconds: f64,
    pub apps: &'a [AppObservation],
    pub cpu_percent: Percent,
    /// The busiest GPU.
    pub gpu_percent: Percent,
    pub hottest_celsius: Option<Celsius>,
    pub pressure_level: PressureLevel,
    pub disk_bytes_per_second: BytesPerSecond,
    pub network_bytes_per_second: BytesPerSecond,
}

impl AlertInput<'_> {
    pub fn app(&self, key: &AppKey) -> Option<&AppObservation> {
        self.apps.iter().find(|app| &app.key == key)
    }
}
