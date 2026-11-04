//! Names and records of the `org.crownos.ProcessMonitor1` interface, shared by
//! the service and [`crate::HelperClient`].

use monitor_system::ProcessAction;
use serde::{Deserialize, Serialize};
use zbus::Type;

pub const BUS_NAME: &str = "org.crownos.ProcessMonitor1";
pub const OBJECT_PATH: &str = "/org/crownos/ProcessMonitor1";
pub const INTERFACE_NAME: &str = "org.crownos.ProcessMonitor1";
/// Bumped whenever a method or record changes incompatibly.
pub const API_VERSION: u32 = 1;

/// All-user figures for one process. Byte counts are cumulative since the
/// process started; `pss`, `uss` and `swap` are 0 until first measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProcessUsage {
    pub pid: u32,
    pub start_ticks: u64,
    pub read_bytes: u64,
    pub written_bytes: u64,
    pub cancelled_written_bytes: u64,
    pub pss: u64,
    pub uss: u64,
    pub swap: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SocketOwner {
    pub inode: u64,
    pub pid: u32,
}

/// The `/proc/<pid>/fdinfo/<fd>` text of an open DRM device.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DrmClient {
    pub pid: u32,
    pub fd: u32,
    pub fdinfo: String,
}

/// A RAPL powercap counter. `zone` is the zone name, prefixed with its
/// parent's for sub-zones (`package-0/core`). The counter wraps at
/// `max_range_microjoules`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct EnergyZone {
    pub zone: String,
    pub energy_microjoules: u64,
    pub max_range_microjoules: u64,
}

/// Bytes of every protocol sent and received by sockets of one cgroup v2,
/// cumulative since the helper started counting. `cgroup_path` is relative
/// to the cgroup root and empty when the cgroup no longer exists.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CgroupTraffic {
    pub cgroup_id: u64,
    pub cgroup_path: String,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
}

/// The signals `Signal` accepts, sent over the bus by [`Self::name`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SignalKind {
    Term,
    Kill,
    Stop,
    Cont,
}

impl SignalKind {
    pub const ALL: [Self; 4] = [Self::Term, Self::Kill, Self::Stop, Self::Cont];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Term => "term",
            Self::Kill => "kill",
            Self::Stop => "stop",
            Self::Cont => "cont",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    pub const fn action(self) -> ProcessAction {
        match self {
            Self::Term => ProcessAction::Quit,
            Self::Kill => ProcessAction::ForceQuit,
            Self::Stop => ProcessAction::Pause,
            Self::Cont => ProcessAction::Resume,
        }
    }
}

impl From<ProcessAction> for SignalKind {
    fn from(action: ProcessAction) -> Self {
        match action {
            ProcessAction::Quit => Self::Term,
            ProcessAction::ForceQuit => Self::Kill,
            ProcessAction::Pause => Self::Stop,
            ProcessAction::Resume => Self::Cont,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_signal_names() {
        for kind in SignalKind::ALL {
            assert_eq!(SignalKind::parse(kind.name()), Some(kind));
            assert_eq!(SignalKind::from(kind.action()), kind);
        }
        assert_eq!(SignalKind::parse("TERM"), None);
        assert_eq!(SignalKind::parse("hup"), None);
        assert_eq!(SignalKind::parse(""), None);
    }

    #[test]
    fn records_have_compact_signatures() {
        assert_eq!(ProcessUsage::SIGNATURE, "(uttttttt)");
        assert_eq!(SocketOwner::SIGNATURE, "(tu)");
        assert_eq!(DrmClient::SIGNATURE, "(uus)");
        assert_eq!(EnergyZone::SIGNATURE, "(stt)");
        assert_eq!(CgroupTraffic::SIGNATURE, "(tstt)");
    }
}
