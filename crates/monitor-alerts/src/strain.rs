use monitor_core::{BytesPerSecond, Celsius, Percent};

use crate::input::{AlertInput, PressureLevel};
use crate::thresholds::Thresholds;

/// The limit a reading is compared against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StrainLimit {
    Percent(Percent),
    Pressure(PressureLevel),
    Celsius(Celsius),
    BytesPerSecond(BytesPerSecond),
}

/// Whether one reading is "red under strain", against which limit and since when.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strain {
    pub limit: StrainLimit,
    pub since_unix: Option<i64>,
}

impl Strain {
    const fn new(limit: StrainLimit) -> Self {
        Self {
            limit,
            since_unix: None,
        }
    }

    pub const fn is_strained(&self) -> bool {
        self.since_unix.is_some()
    }

    pub fn seconds(&self, now_unix: i64) -> i64 {
        self.since_unix.map_or(0, |since| (now_unix - since).max(0))
    }

    fn update(&mut self, limit: StrainLimit, over: bool, now_unix: i64) {
        self.limit = limit;
        self.since_unix = if over {
            Some(self.since_unix.unwrap_or(now_unix))
        } else {
            None
        };
    }
}

/// Live strain per reading, using the same thresholds as alerts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrainState {
    pub cpu: Strain,
    pub memory: Strain,
    pub gpu: Strain,
    pub temperature: Strain,
    pub disk: Strain,
    pub network: Strain,
}

impl Default for StrainState {
    fn default() -> Self {
        Self::new(&Thresholds::default())
    }
}

impl StrainState {
    pub fn new(thresholds: &Thresholds) -> Self {
        Self {
            cpu: Strain::new(StrainLimit::Percent(thresholds.strain_cpu)),
            memory: Strain::new(StrainLimit::Pressure(PressureLevel::Elevated)),
            gpu: Strain::new(StrainLimit::Percent(thresholds.strain_gpu)),
            temperature: Strain::new(StrainLimit::Celsius(thresholds.hot)),
            disk: Strain::new(StrainLimit::BytesPerSecond(thresholds.strain_disk)),
            network: Strain::new(StrainLimit::BytesPerSecond(thresholds.strain_network)),
        }
    }

    pub fn update(&mut self, input: &AlertInput<'_>, thresholds: &Thresholds) {
        let now = input.wall_unix_seconds;
        self.cpu.update(
            StrainLimit::Percent(thresholds.strain_cpu),
            input.cpu_percent >= thresholds.strain_cpu,
            now,
        );
        self.memory.update(
            StrainLimit::Pressure(PressureLevel::Elevated),
            input.pressure_level >= PressureLevel::Elevated,
            now,
        );
        self.gpu.update(
            StrainLimit::Percent(thresholds.strain_gpu),
            input.gpu_percent >= thresholds.strain_gpu,
            now,
        );
        self.temperature.update(
            StrainLimit::Celsius(thresholds.hot),
            input.hottest_celsius.is_some_and(|c| c >= thresholds.hot),
            now,
        );
        self.disk.update(
            StrainLimit::BytesPerSecond(thresholds.strain_disk),
            input.disk_bytes_per_second >= thresholds.strain_disk,
            now,
        );
        self.network.update(
            StrainLimit::BytesPerSecond(thresholds.strain_network),
            input.network_bytes_per_second >= thresholds.strain_network,
            now,
        );
    }
}
