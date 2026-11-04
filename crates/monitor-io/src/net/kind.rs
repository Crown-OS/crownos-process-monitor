//! What an interface is, from its rtnetlink link kind and sysfs hints.

/// The role of a network interface. Only [`InterfaceKind::counts_in_totals`]
/// kinds add to the download/upload totals:
///
/// - `Wifi`, `Ethernet`, `Mobile`: physical uplinks.
/// - `Other`: a physical device of an unrecognised type (InfiniBand, CAN…).
/// - `Vpn`: tun, WireGuard, Tailscale, IP tunnels. Their traffic also
///   crosses a physical NIC, so it is shown separately to avoid double counting.
/// - `Bridge`: Linux bridges (docker0, virbr0, waydroid0).
/// - `Virtual`: veth, tap, dummy, VLAN, bond and other software devices.
/// - `Loopback`: `lo`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum InterfaceKind {
    Wifi,
    Ethernet,
    Vpn,
    Bridge,
    Virtual,
    Loopback,
    Mobile,
    #[default]
    Other,
}

impl InterfaceKind {
    pub const fn counts_in_totals(self) -> bool {
        matches!(
            self,
            Self::Wifi | Self::Ethernet | Self::Mobile | Self::Other
        )
    }

    pub const fn is_tunnel(self) -> bool {
        matches!(self, Self::Vpn)
    }
}

const ARPHRD_ETHER: u16 = 1;
const ARPHRD_RAWIP: u16 = 519;
const ARPHRD_LOOPBACK: u16 = 772;
const ARPHRD_NONE: u16 = 65534;
const IFF_TUN: u32 = 0x0001;
const TUN_TYPE_MASK: u32 = 0x000f;
const TUNNEL_LINK_KINDS: &[&str] = &[
    "wireguard",
    "ipip",
    "ip6tnl",
    "sit",
    "gre",
    "ip6gre",
    "vti",
    "vti6",
    "xfrm",
    "amt",
    "gtp",
    "ipoib",
];
const MOBILE_LINK_KINDS: &[&str] = &["wwan", "rmnet"];

/// Everything the classifier looks at, gathered once per refresh.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KindHints<'a> {
    /// `IFLA_INFO_KIND`, e.g. `veth`, `bridge`, `wireguard`.
    pub link_kind: Option<&'a str>,
    /// `/sys/class/net/X/type` (an `ARPHRD_*` value).
    pub hardware_type: Option<u16>,
    /// `DEVTYPE` from `uevent`, e.g. `wlan`, `bridge`, `wwan`.
    pub device_type: Option<&'a str>,
    pub has_wireless: bool,
    pub has_bridge: bool,
    pub tun_flags: Option<u32>,
    /// The device lives under `/sys/devices/virtual/net`.
    pub is_virtual: bool,
}

pub(crate) fn classify(hints: &KindHints<'_>) -> InterfaceKind {
    let link_kind = hints.link_kind.unwrap_or_default();
    let device_type = hints.device_type.unwrap_or_default();
    let is_tun = hints
        .tun_flags
        .is_some_and(|flags| flags & TUN_TYPE_MASK == IFF_TUN);
    if hints.hardware_type == Some(ARPHRD_LOOPBACK) {
        InterfaceKind::Loopback
    } else if hints.has_wireless || device_type == "wlan" {
        InterfaceKind::Wifi
    } else if device_type == "wwan"
        || MOBILE_LINK_KINDS.contains(&link_kind)
        || hints.hardware_type == Some(ARPHRD_RAWIP)
    {
        InterfaceKind::Mobile
    } else if hints.has_bridge || device_type == "bridge" || link_kind == "bridge" {
        InterfaceKind::Bridge
    } else if is_tun
        || TUNNEL_LINK_KINDS.contains(&link_kind)
        || device_type == "wireguard"
        || (hints.is_virtual && hints.hardware_type == Some(ARPHRD_NONE))
    {
        InterfaceKind::Vpn
    } else if hints.is_virtual || !link_kind.is_empty() {
        InterfaceKind::Virtual
    } else if hints.hardware_type == Some(ARPHRD_ETHER) {
        InterfaceKind::Ethernet
    } else {
        InterfaceKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_this_machines_interfaces() {
        let wifi = KindHints {
            hardware_type: Some(1),
            device_type: Some("wlan"),
            has_wireless: true,
            ..KindHints::default()
        };
        let tailscale = KindHints {
            link_kind: Some("tun"),
            hardware_type: Some(65534),
            tun_flags: Some(0x5001),
            is_virtual: true,
            ..KindHints::default()
        };
        let waydroid = KindHints {
            link_kind: Some("bridge"),
            hardware_type: Some(1),
            device_type: Some("bridge"),
            has_bridge: true,
            is_virtual: true,
            ..KindHints::default()
        };
        let veth = KindHints {
            link_kind: Some("veth"),
            hardware_type: Some(1),
            is_virtual: true,
            ..KindHints::default()
        };
        let ethernet = KindHints {
            hardware_type: Some(1),
            ..KindHints::default()
        };
        let loopback = KindHints {
            hardware_type: Some(772),
            is_virtual: true,
            ..KindHints::default()
        };
        let tap = KindHints {
            link_kind: Some("tun"),
            hardware_type: Some(1),
            tun_flags: Some(0x1002),
            is_virtual: true,
            ..KindHints::default()
        };
        assert_eq!(classify(&wifi), InterfaceKind::Wifi);
        assert_eq!(classify(&tailscale), InterfaceKind::Vpn);
        assert_eq!(classify(&waydroid), InterfaceKind::Bridge);
        assert_eq!(classify(&veth), InterfaceKind::Virtual);
        assert_eq!(classify(&ethernet), InterfaceKind::Ethernet);
        assert_eq!(classify(&loopback), InterfaceKind::Loopback);
        assert_eq!(
            classify(&tap),
            InterfaceKind::Virtual,
            "VM taps are not tunnels"
        );
    }

    #[test]
    fn only_uplinks_count_in_totals() {
        assert!(InterfaceKind::Wifi.counts_in_totals());
        assert!(InterfaceKind::Ethernet.counts_in_totals());
        assert!(!InterfaceKind::Vpn.counts_in_totals());
        assert!(!InterfaceKind::Bridge.counts_in_totals());
        assert!(!InterfaceKind::Virtual.counts_in_totals());
        assert!(!InterfaceKind::Loopback.counts_in_totals());
    }
}
