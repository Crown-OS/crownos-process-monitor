use monitor_core::util::time::HOUR_SECONDS;
use monitor_core::{AppKey, Bytes, BytesPerSecond, Celsius, Percent};

use crate::alert::AlertKind;

const MEGABYTE: f64 = 1_000_000.0;
const GIBIBYTE: u64 = 1 << 30;

/// Detection limits; `Default` follows the plan. Durations are whole
/// minutes because rules run on per-minute averages.
#[derive(Clone, Debug, PartialEq)]
pub struct Thresholds {
    pub sustained_cpu: Percent,
    pub sustained_cpu_minutes: u32,
    pub memory_growth: Bytes,
    pub memory_growth_minutes: u32,
    /// A burst is this many times the app's usual rate…
    pub burst_factor: f64,
    /// …and at least this fast.
    pub disk_burst_floor: BytesPerSecond,
    pub network_burst_floor: BytesPerSecond,
    /// Minutes of history before an app's baseline is trusted.
    pub burst_warmup_minutes: u32,
    pub hot: Celsius,
    pub hot_minutes: u32,
    pub busy_gpu: Percent,
    pub busy_gpu_minutes: u32,
    pub cooldown_seconds: i64,
    pub strain_cpu: Percent,
    pub strain_gpu: Percent,
    pub strain_disk: BytesPerSecond,
    pub strain_network: BytesPerSecond,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            sustained_cpu: Percent::new(70.0),
            sustained_cpu_minutes: 10,
            memory_growth: Bytes::new(GIBIBYTE),
            memory_growth_minutes: 60,
            burst_factor: 10.0,
            disk_burst_floor: BytesPerSecond::new(50.0 * MEGABYTE),
            network_burst_floor: BytesPerSecond::new(10.0 * MEGABYTE),
            burst_warmup_minutes: 10,
            hot: Celsius::new(90.0),
            hot_minutes: 5,
            busy_gpu: Percent::new(90.0),
            busy_gpu_minutes: 10,
            cooldown_seconds: HOUR_SECONDS,
            strain_cpu: Percent::new(90.0),
            strain_gpu: Percent::new(90.0),
            strain_disk: BytesPerSecond::new(500.0 * MEGABYTE),
            strain_network: BytesPerSecond::new(100.0 * MEGABYTE),
        }
    }
}

/// A user-set per-app limit, e.g. "alert when > 8 GB for 5 min".
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppLimit {
    Memory(Bytes),
    Cpu(Percent),
}

impl AppLimit {
    pub const fn kind(self) -> AlertKind {
        match self {
            Self::Memory(_) => AlertKind::MemoryLimit,
            Self::Cpu(_) => AlertKind::CpuLimit,
        }
    }

    pub fn value(self) -> f64 {
        match self {
            Self::Memory(bytes) => bytes.get() as f64,
            Self::Cpu(percent) => f64::from(percent.get()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CustomLimit {
    pub app: AppKey,
    pub limit: AppLimit,
    pub minutes: u32,
}
