use std::path::Path;
use std::sync::Arc;

use monitor_core::{Celsius, Percent, Rpm};

/// What a sensor measures, decided from its hwmon driver name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SensorCategory {
    Cpu,
    Gpu,
    Drive,
    /// Embedded controller and Super I/O chips (laptop ECs, nct*, it87).
    Board,
    Wireless,
    Battery,
    /// ACPI thermal zones, the fallback when nothing better exists.
    Acpi,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SensorIdentity {
    /// Stable across boots: `<driver>@<device path>/<channel>`.
    pub key: Arc<str>,
    /// Driver name, or the device name for multi-instance drivers (`nvme1`).
    pub chip: Arc<str>,
    pub label: Arc<str>,
    pub category: SensorCategory,
    pub high: Option<Celsius>,
    pub critical: Option<Celsius>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TemperatureReading {
    pub identity: Arc<SensorIdentity>,
    /// `None` for sentinel values (-273, 0, 127, 255 °C) or a sleeping device.
    pub value: Option<Celsius>,
}

/// Identifies a fan for [`crate::sensors::FanControl`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FanId {
    pub chip_directory: Arc<Path>,
    pub channel: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FanControlSupport {
    ReadOnly,
    /// `pwmN` and `pwmN_enable` exist; writing them needs the helper.
    Pwm,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FanIdentity {
    pub id: FanId,
    pub key: Arc<str>,
    pub chip: Arc<str>,
    pub label: Arc<str>,
    pub min: Option<Rpm>,
    pub max: Option<Rpm>,
    pub control: FanControlSupport,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FanReading {
    pub identity: Arc<FanIdentity>,
    pub rpm: Option<Rpm>,
    pub duty: Option<Percent>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SensorSnapshot {
    /// The best CPU package/die sensor available.
    pub cpu_temperature: Option<Celsius>,
    /// Index into `temperatures` of the hottest reading.
    pub hottest: Option<usize>,
    pub temperatures: Vec<TemperatureReading>,
    pub fans: Vec<FanReading>,
    /// Sum of Intel core throttle events plus package events; `None` when
    /// the CPU exposes no counters.
    pub throttle_events: Option<u64>,
}

impl SensorSnapshot {
    pub fn hottest_sensor(&self) -> Option<&TemperatureReading> {
        self.temperatures.get(self.hottest?)
    }
}
