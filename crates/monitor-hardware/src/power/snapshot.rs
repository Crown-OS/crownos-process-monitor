use std::sync::Arc;
use std::time::Duration;

use monitor_core::{Coverage, Percent, Pid, Uid, Watts};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PowerSource {
    Ac,
    Battery,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BatteryStatus {
    Charging,
    Discharging,
    NotCharging,
    Full,
    #[default]
    Unknown,
}

impl BatteryStatus {
    pub fn parse(text: &[u8]) -> Self {
        match text {
            b"Charging" => Self::Charging,
            b"Discharging" => Self::Discharging,
            b"Not charging" => Self::NotCharging,
            b"Full" => Self::Full,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct WattHours(f32);

impl WattHours {
    pub const fn new(watt_hours: f32) -> Self {
        Self(watt_hours)
    }

    pub fn from_microwatt_hours(micro: u64) -> Self {
        Self(micro as f32 / 1_000_000.0)
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Whether a battery powers the machine or a peripheral (mouse, headset).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BatteryScope {
    #[default]
    System,
    Device,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatteryIdentity {
    pub name: Arc<str>,
    pub model: Option<Arc<str>>,
    pub manufacturer: Option<Arc<str>>,
    pub technology: Option<Arc<str>>,
    pub scope: BatteryScope,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatteryReading {
    pub identity: Arc<BatteryIdentity>,
    pub status: BatteryStatus,
    pub capacity: Option<Percent>,
    /// Charge or discharge rate, always positive.
    pub power: Option<Watts>,
    pub energy_now: Option<WattHours>,
    pub energy_full: Option<WattHours>,
    pub energy_full_design: Option<WattHours>,
    /// Full capacity relative to design capacity.
    pub health: Option<Percent>,
    pub cycle_count: Option<u32>,
    pub time_to_empty: Option<Duration>,
    pub time_to_full: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemDraw {
    pub watts: Watts,
    /// `Full` when measured at the battery, `Estimated` when summed from
    /// RAPL and GPU sensors.
    pub coverage: Coverage,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RaplPower {
    /// Powercap zone name: `package-0`, `core`, `uncore`, `dram`, `psys`.
    pub name: Arc<str>,
    pub is_package: bool,
    pub watts: Watts,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlatformProfile {
    pub active: Arc<str>,
    pub choices: Arc<[Arc<str>]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InhibitMode {
    Block,
    Delay,
    Other,
}

/// A logind inhibitor lock: who keeps the machine from sleeping.
#[derive(Clone, Debug, PartialEq)]
pub struct Inhibitor {
    /// Colon-separated: `sleep`, `shutdown`, `idle`, `handle-lid-switch`…
    pub what: Arc<str>,
    pub who: Arc<str>,
    pub why: Arc<str>,
    pub mode: InhibitMode,
    pub uid: Uid,
    pub pid: Pid,
    /// logind does not report when a lock was taken; this is when the
    /// monitor first saw it.
    pub first_seen_unix: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PowerSnapshot {
    pub source: PowerSource,
    pub batteries: Vec<BatteryReading>,
    pub system_draw: Option<SystemDraw>,
    pub rapl: Vec<RaplPower>,
    /// `Unavailable` without root (powercap `energy_uj` is 0400).
    pub rapl_coverage: Coverage,
    pub platform_profile: Option<PlatformProfile>,
    /// power-profiles-daemon's `ActiveProfile`.
    pub daemon_profile: Option<Arc<str>>,
    pub inhibitors: Arc<[Inhibitor]>,
    /// `Unavailable` until the system bus answered.
    pub dbus_coverage: Coverage,
}

impl PowerSnapshot {
    pub fn system_battery(&self) -> Option<&BatteryReading> {
        self.batteries
            .iter()
            .find(|battery| battery.identity.scope == BatteryScope::System)
    }
}
