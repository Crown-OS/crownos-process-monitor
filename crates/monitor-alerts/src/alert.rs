use std::sync::Arc;

use monitor_core::AppKey;

use crate::input::AlertInput;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AlertKind {
    SustainedCpu,
    MemoryGrowth,
    DiskBurst,
    NetworkBurst,
    MachineHot,
    GpuBusy,
    MemoryLimit,
    CpuLimit,
}

/// How an alert's `value` and `limit` are measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlertUnit {
    Percent,
    Bytes,
    BytesPerSecond,
    Celsius,
}

impl AlertKind {
    pub const ALL: [Self; 8] = [
        Self::SustainedCpu,
        Self::MemoryGrowth,
        Self::DiskBurst,
        Self::NetworkBurst,
        Self::MachineHot,
        Self::GpuBusy,
        Self::MemoryLimit,
        Self::CpuLimit,
    ];

    /// Stable identifier for persistence.
    pub const fn code(self) -> &'static str {
        match self {
            Self::SustainedCpu => "sustained_cpu",
            Self::MemoryGrowth => "memory_growth",
            Self::DiskBurst => "disk_burst",
            Self::NetworkBurst => "network_burst",
            Self::MachineHot => "machine_hot",
            Self::GpuBusy => "gpu_busy",
            Self::MemoryLimit => "memory_limit",
            Self::CpuLimit => "cpu_limit",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }

    pub const fn unit(self) -> AlertUnit {
        match self {
            Self::SustainedCpu | Self::GpuBusy | Self::CpuLimit => AlertUnit::Percent,
            Self::MemoryGrowth | Self::MemoryLimit => AlertUnit::Bytes,
            Self::DiskBurst | Self::NetworkBurst => AlertUnit::BytesPerSecond,
            Self::MachineHot => AlertUnit::Celsius,
        }
    }

    /// The live figure this kind watches, or `None` when the app isn't
    /// running or the sensor is missing.
    pub fn current_value(self, app: Option<&AppKey>, input: &AlertInput<'_>) -> Option<f64> {
        match self {
            Self::MachineHot => input.hottest_celsius.map(|c| f64::from(c.get())),
            Self::GpuBusy => Some(f64::from(input.gpu_percent.get())),
            _ => {
                let app = input.app(app?)?;
                Some(match self {
                    Self::MemoryGrowth | Self::MemoryLimit => app.memory.get() as f64,
                    Self::DiskBurst => app.disk_bytes_per_second.get(),
                    Self::NetworkBurst => app.network_bytes_per_second.get(),
                    _ => f64::from(app.cpu_percent.get()),
                })
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AlertApp {
    pub key: AppKey,
    pub name: Arc<str>,
}

/// A rule fired. `value` and `limit` are in [`AlertKind::unit`]; for memory
/// growth `value` is the growth over the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub kind: AlertKind,
    /// `None` for machine-wide alerts.
    pub app: Option<AlertApp>,
    pub at_unix: i64,
    pub value: f64,
    pub limit: f64,
}

/// A Worth a Look row as plain data, for persistence through
/// `monitor_history::StoredAlert` (the engine copies the fields and maps
/// `kind` with [`AlertKind::code`] / [`AlertKind::from_code`]).
#[derive(Clone, Debug, PartialEq)]
pub struct AlertRecord {
    pub kind: AlertKind,
    pub app: Option<AlertApp>,
    pub start_unix: i64,
    pub end_unix: i64,
    pub peak: f64,
    pub limit: f64,
    pub count: u32,
    pub quieted: bool,
    pub pinned: bool,
}

/// Inputs for a row's "now" text, e.g. "Firefox is using 2.1 GB now".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurrentState {
    pub value: Option<f64>,
    pub over_limit: bool,
}

impl AlertRecord {
    pub fn app_key(&self) -> Option<&AppKey> {
        self.app.as_ref().map(|app| &app.key)
    }

    pub fn current_state(&self, input: &AlertInput<'_>) -> CurrentState {
        let value = self.kind.current_value(self.app_key(), input);
        CurrentState {
            value,
            over_limit: self.kind != AlertKind::MemoryGrowth
                && value.is_some_and(|value| value >= self.limit),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        assert!(
            AlertKind::ALL
                .into_iter()
                .all(|kind| AlertKind::from_code(kind.code()) == Some(kind))
        );
        assert_eq!(AlertKind::from_code("nope"), None);
    }
}
