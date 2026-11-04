/// Everything the helper can do on a client's behalf. Each D-Bus method maps
/// to exactly one capability, and authorization backends only ever see these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Capability {
    ReadAllProcesses,
    ReadEnergy,
    ReadNetworkAccounting,
    ReadGpuCounters,
    ControlFans,
    SignalOtherUsers,
}

impl Capability {
    pub const ALL: [Self; 6] = [
        Self::ReadAllProcesses,
        Self::ReadEnergy,
        Self::ReadNetworkAccounting,
        Self::ReadGpuCounters,
        Self::ControlFans,
        Self::SignalOtherUsers,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::ReadAllProcesses => "read-all-processes",
            Self::ReadEnergy => "read-energy",
            Self::ReadNetworkAccounting => "read-network-accounting",
            Self::ReadGpuCounters => "read-gpu-counters",
            Self::ControlFans => "control-fans",
            Self::SignalOtherUsers => "signal-other-users",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|capability| capability.name() == name)
    }

    /// Reading figures never changes the system; such requests may be
    /// granted without asking the person.
    pub const fn is_read_only(self) -> bool {
        matches!(
            self,
            Self::ReadAllProcesses
                | Self::ReadEnergy
                | Self::ReadNetworkAccounting
                | Self::ReadGpuCounters
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for capability in Capability::ALL {
            assert_eq!(Capability::from_name(capability.name()), Some(capability));
        }
        assert_eq!(Capability::from_name("bogus"), None);
    }

    #[test]
    fn only_reads_are_read_only() {
        assert!(Capability::ReadEnergy.is_read_only());
        assert!(!Capability::ControlFans.is_read_only());
        assert!(!Capability::SignalOtherUsers.is_read_only());
    }
}
