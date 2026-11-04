use crate::Capability;

/// Every method of `org.crownos.ProcessMonitor1`. The service authorizes a
/// call through [`Method::capability`] and nothing else, so this match is
/// the whole privilege map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    ProcessUsage,
    SocketOwners,
    DrmClients,
    Energy,
    NetworkAccounting,
    SetFan,
    RestoreFans,
    Signal,
    Renice,
}

impl Method {
    pub const ALL: [Self; 9] = [
        Self::ProcessUsage,
        Self::SocketOwners,
        Self::DrmClients,
        Self::Energy,
        Self::NetworkAccounting,
        Self::SetFan,
        Self::RestoreFans,
        Self::Signal,
        Self::Renice,
    ];

    pub const fn capability(self) -> Capability {
        match self {
            Self::ProcessUsage | Self::SocketOwners => Capability::ReadAllProcesses,
            Self::DrmClients => Capability::ReadGpuCounters,
            Self::Energy => Capability::ReadEnergy,
            Self::NetworkAccounting => Capability::ReadNetworkAccounting,
            Self::SetFan | Self::RestoreFans => Capability::ControlFans,
            Self::Signal | Self::Renice => Capability::SignalOtherUsers,
        }
    }

    /// The D-Bus member name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::ProcessUsage => "ProcessUsage",
            Self::SocketOwners => "SocketOwners",
            Self::DrmClients => "DrmClients",
            Self::Energy => "Energy",
            Self::NetworkAccounting => "NetworkAccounting",
            Self::SetFan => "SetFan",
            Self::RestoreFans => "RestoreFans",
            Self::Signal => "Signal",
            Self::Renice => "Renice",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|method| method.name() == name)
    }
}

/// The capability a D-Bus method needs, `None` for names that are not
/// methods of the interface.
pub fn capability_for(method_name: &str) -> Option<Capability> {
    Method::from_name(method_name).map(Method::capability)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_method_maps_to_one_capability() {
        let names: HashSet<_> = Method::ALL.iter().map(|method| method.name()).collect();
        assert_eq!(names.len(), Method::ALL.len(), "member names are unique");
        for method in Method::ALL {
            assert_eq!(capability_for(method.name()), Some(method.capability()));
        }
        assert_eq!(capability_for("Bogus"), None);
        assert_eq!(capability_for("processusage"), None);
    }

    #[test]
    fn every_capability_is_reachable() {
        let used: HashSet<_> = Method::ALL.into_iter().map(Method::capability).collect();
        assert!(
            Capability::ALL
                .iter()
                .all(|capability| used.contains(capability))
        );
    }

    #[test]
    fn writes_need_write_capabilities() {
        assert_eq!(
            capability_for("ProcessUsage"),
            Some(Capability::ReadAllProcesses)
        );
        assert_eq!(
            capability_for("DrmClients"),
            Some(Capability::ReadGpuCounters)
        );
        assert_eq!(capability_for("Energy"), Some(Capability::ReadEnergy));
        assert_eq!(
            capability_for("NetworkAccounting"),
            Some(Capability::ReadNetworkAccounting)
        );
        for write in [
            Method::SetFan,
            Method::RestoreFans,
            Method::Signal,
            Method::Renice,
        ] {
            assert!(!write.capability().is_read_only(), "{write:?}");
        }
        assert_eq!(capability_for("Renice"), Some(Capability::SignalOtherUsers));
        assert_eq!(capability_for("RestoreFans"), Some(Capability::ControlFans));
    }
}
