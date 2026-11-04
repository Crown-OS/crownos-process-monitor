use std::sync::Arc;

pub use monitor_core::PressureLevel;
use monitor_core::{AppKey, Bytes, Celsius, DeviceKey, Percent, Rpm, Watts};

use crate::gpu_engine::EngineBusy;

/// Machine-wide figures for one sampler tick. Byte and energy fields are
/// deltas since the previous tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemSample {
    pub wall_unix_seconds: i64,
    /// Monotonic seconds since the previous tick.
    pub elapsed_seconds: f64,
    /// The machine slept since the previous tick: the sample is ignored.
    pub suspended: bool,
    /// Busy CPU time in *machine* seconds: the sum over all logical CPUs
    /// divided by their count, so a fully busy machine adds 1.0 per second.
    pub cpu_busy_seconds: f64,
    pub cpu_percent: Percent,
    pub memory_used: Bytes,
    pub memory_pressure_percent: Percent,
    pub pressure_level: PressureLevel,
    pub swap_used: Bytes,
    /// The busiest GPU.
    pub gpu_percent: Percent,
    pub npu_percent: Option<Percent>,
    pub cpu_celsius: Option<Celsius>,
    pub hottest_celsius: Option<Celsius>,
    pub fan_rpm: Option<Rpm>,
    pub power_watts: Option<Watts>,
    pub battery_percent: Option<Percent>,
    pub on_ac: Option<bool>,
    pub throttled: bool,
    pub network_received: Bytes,
    pub network_transmitted: Bytes,
    pub disk_read: Bytes,
    pub disk_written: Bytes,
    pub energy_joules: Option<f64>,
}

/// One application's figures for one tick; the kind comes from `key.kind()`.
#[derive(Clone, Debug, PartialEq)]
pub struct AppSample {
    pub key: AppKey,
    pub name: Arc<str>,
    /// CPU time consumed during the tick, in core-seconds (utime + stime).
    pub cpu_seconds: f64,
    pub memory: Bytes,
    pub disk_read: Bytes,
    pub disk_written: Bytes,
    pub network_received: Bytes,
    pub network_transmitted: Bytes,
    pub gpu_seconds: f64,
    pub energy_joules: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuSample {
    pub key: DeviceKey,
    pub busy_seconds: f64,
    pub engine_busy_seconds: EngineBusy,
    pub vram_used: Bytes,
    pub energy_joules: Option<f64>,
    pub celsius: Option<Celsius>,
}

/// Bytes moved by one block device; a tick delta as input, a sum in batches.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceSample {
    pub key: DeviceKey,
    pub read: Bytes,
    pub written: Bytes,
}

/// Bytes moved by one network interface; a tick delta as input, a sum in batches.
#[derive(Clone, Debug, PartialEq)]
pub struct InterfaceSample {
    pub key: DeviceKey,
    pub received: Bytes,
    pub transmitted: Bytes,
}

/// Since-boot kernel totals, persisted each minute so the next start can
/// estimate what happened while the app was closed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KernelCounters {
    pub taken_unix_seconds: i64,
    pub boot_id: Arc<str>,
    /// Machine seconds, normalised like [`SystemSample::cpu_busy_seconds`].
    pub cpu_busy_seconds_total: f64,
    pub disk_read_total: Bytes,
    pub disk_written_total: Bytes,
    pub network_received_total: Bytes,
    pub network_transmitted_total: Bytes,
}

/// Everything the engine measured in one tick, borrowed for the accumulator.
#[derive(Clone, Copy, Debug)]
pub struct TickSamples<'a> {
    pub system: &'a SystemSample,
    pub apps: &'a [AppSample],
    pub gpus: &'a [GpuSample],
    pub devices: &'a [DeviceSample],
    pub interfaces: &'a [InterfaceSample],
}
