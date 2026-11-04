use std::sync::Arc;

use monitor_core::{AppKey, Bytes, DeviceKey};

use crate::gauge::Gauges;
use crate::gpu_engine::EngineTimes;
use crate::sample::{DeviceSample, InterfaceSample, PressureLevel};

/// Machine figures summed over one UTC-aligned minute.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemMinute {
    pub start_unix: i64,
    pub last_sample_unix: i64,
    pub recorded_seconds: f64,
    pub cpu_busy_seconds: f64,
    pub memory_byte_seconds: f64,
    pub disk_read: Bytes,
    pub disk_written: Bytes,
    pub network_received: Bytes,
    pub network_transmitted: Bytes,
    pub energy_joules: Option<f64>,
    pub gauges: Gauges,
    pub pressure_level_peak: PressureLevel,
    pub throttled_seconds: f64,
    pub on_ac_seconds: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppMinute {
    pub key: AppKey,
    pub name: Arc<str>,
    /// Seconds the app was observed in the minute.
    pub seconds: f64,
    pub cpu_seconds: f64,
    pub memory_byte_seconds: f64,
    pub peak_memory: Bytes,
    pub disk_read: Bytes,
    pub disk_written: Bytes,
    pub network_received: Bytes,
    pub network_transmitted: Bytes,
    pub gpu_seconds: f64,
    pub energy_joules: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuMinute {
    pub key: DeviceKey,
    pub seconds: f64,
    pub busy_seconds: f64,
    pub engines: EngineTimes,
    pub vram_byte_seconds: f64,
    pub energy_joules: Option<f64>,
    pub peak_celsius: Option<f32>,
}

/// One finished minute, handed from the accumulator to the history writer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RollupBatch {
    pub system: SystemMinute,
    pub apps: Vec<AppMinute>,
    pub gpus: Vec<GpuMinute>,
    pub devices: Vec<DeviceSample>,
    pub interfaces: Vec<InterfaceSample>,
}

pub(crate) fn add_optional(total: &mut Option<f64>, value: Option<f64>) {
    if let Some(value) = value {
        *total = Some(total.unwrap_or(0.0) + value);
    }
}
