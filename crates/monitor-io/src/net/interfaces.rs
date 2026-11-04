//! Interface facts that change only on events: names, kinds, MAC, MTU,
//! speed, addresses and default routes. Built from rtnetlink dumps (falling
//! back to sysfs) whenever a multicast event arrives; allocation is fine here.

use std::fmt::Write;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;

use monitor_core::util::{parse, sysfs};
use netlink_packet_core::Parseable;
use netlink_packet_route::address::{AddressAttribute, AddressMessage};
use netlink_packet_route::link::{LinkAttribute, LinkInfo, LinkMessage};
use netlink_packet_route::route::{RouteAddress, RouteAttribute, RouteMessage};

use super::kind::{self, InterfaceKind, KindHints};
use crate::util::text::read_attribute;

const SYS_CLASS_NET: &str = "/sys/class/net";
const VIRTUAL_NET: &str = "/sys/devices/virtual/net/";
const MAIN_ROUTE_TABLE: u32 = 254;
const UNKNOWN_SPEED: i64 = -1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Duplex {
    Full,
    Half,
    #[default]
    Unknown,
}

/// RFC 2863 operational state (`IF_OPER_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OperState {
    #[default]
    Unknown,
    NotPresent,
    Down,
    LowerLayerDown,
    Testing,
    Dormant,
    Up,
}

impl OperState {
    pub(crate) fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::NotPresent,
            2 => Self::Down,
            3 => Self::LowerLayerDown,
            4 => Self::Testing,
            5 => Self::Dormant,
            6 => Self::Up,
            _ => Self::Unknown,
        }
    }

    fn from_sysfs(text: &str) -> Self {
        match text {
            "notpresent" => Self::NotPresent,
            "down" => Self::Down,
            "lowerlayerdown" => Self::LowerLayerDown,
            "testing" => Self::Testing,
            "dormant" => Self::Dormant,
            "up" => Self::Up,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InterfaceAddress {
    pub address: IpAddr,
    pub prefix_length: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceInfo {
    pub index: u32,
    pub name: Arc<str>,
    pub kind: InterfaceKind,
    /// The rtnetlink link kind (`veth`, `bridge`, `tun`, `wireguard`…).
    pub link_kind: Option<Arc<str>>,
    pub mac: Option<Arc<str>>,
    pub mtu: Option<u32>,
    /// Negotiated link speed; `None` for Wi-Fi, virtual or down links.
    pub speed_mbps: Option<u32>,
    pub duplex: Duplex,
    pub addresses: Vec<InterfaceAddress>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultRoute {
    pub gateway: IpAddr,
    pub interface_index: u32,
    pub interface: Option<Arc<str>>,
}

/// What a link dump (or sysfs) says about one interface.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LinkRecord {
    pub index: u32,
    pub name: String,
    pub mac: Option<String>,
    pub mtu: Option<u32>,
    pub link_kind: Option<String>,
    pub operstate: OperState,
}

pub(crate) fn link_record(payload: &[u8]) -> Option<LinkRecord> {
    let message = LinkMessage::parse(payload).ok()?;
    let mut record = LinkRecord {
        index: message.header.index,
        ..LinkRecord::default()
    };
    for attribute in message.attributes {
        match attribute {
            LinkAttribute::IfName(name) => record.name = name,
            LinkAttribute::Address(bytes) => record.mac = format_mac(&bytes),
            LinkAttribute::Mtu(mtu) => record.mtu = Some(mtu),
            LinkAttribute::OperState(state) => {
                record.operstate = OperState::from_raw(u8::from(state))
            }
            LinkAttribute::LinkInfo(infos) => {
                record.link_kind = infos.into_iter().find_map(|info| match info {
                    LinkInfo::Kind(kind) => Some(kind.to_string()),
                    _ => None,
                });
            }
            _ => {}
        }
    }
    (!record.name.is_empty()).then_some(record)
}

/// Fallback listing when rtnetlink is unavailable.
pub(crate) fn sysfs_records() -> Vec<LinkRecord> {
    let Ok(entries) = std::fs::read_dir(SYS_CLASS_NET) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let directory = entry.path();
            let index = sysfs::read_small(directory.join("ifindex"), parse::parse_u32).ok()?;
            Some(LinkRecord {
                index,
                mac: read_attribute(directory.join("address"))
                    .filter(|mac| mac.bytes().any(|byte| byte != b'0' && byte != b':'))
                    .map(|mac| mac.to_string()),
                mtu: sysfs::read_small(directory.join("mtu"), parse::parse_u32).ok(),
                link_kind: None,
                operstate: read_attribute(directory.join("operstate"))
                    .map_or(OperState::Unknown, |state| OperState::from_sysfs(&state)),
                name,
            })
        })
        .collect()
}

/// Completes a link record with sysfs details and its addresses.
pub(crate) fn describe(
    record: &LinkRecord,
    addresses: &[(u32, InterfaceAddress)],
) -> InterfaceInfo {
    let directory = Path::new(SYS_CLASS_NET).join(&record.name);
    let uevent = read_attribute(directory.join("uevent"));
    let device_type = uevent
        .as_deref()
        .and_then(|text| text.lines().find_map(|line| line.strip_prefix("DEVTYPE=")));
    let hints = KindHints {
        link_kind: record.link_kind.as_deref(),
        hardware_type: sysfs::read_small(directory.join("type"), |bytes| {
            parse::parse_u32(bytes).and_then(|value| u16::try_from(value).ok())
        })
        .ok(),
        device_type,
        has_wireless: directory.join("wireless").exists() || directory.join("phy80211").exists(),
        has_bridge: directory.join("bridge").exists(),
        tun_flags: sysfs::read_small(directory.join("tun_flags"), parse::parse_hex_u32).ok(),
        is_virtual: std::fs::canonicalize(&directory)
            .is_ok_and(|real| real.starts_with(VIRTUAL_NET)),
    };
    InterfaceInfo {
        index: record.index,
        name: Arc::from(record.name.as_str()),
        kind: kind::classify(&hints),
        link_kind: record.link_kind.as_deref().map(Arc::from),
        mac: record.mac.as_deref().map(Arc::from),
        mtu: record.mtu,
        speed_mbps: sysfs::read_i64(directory.join("speed"))
            .ok()
            .filter(|speed| *speed > 0 && *speed != UNKNOWN_SPEED)
            .and_then(|speed| u32::try_from(speed).ok()),
        duplex: match read_attribute(directory.join("duplex")).as_deref() {
            Some("full") => Duplex::Full,
            Some("half") => Duplex::Half,
            _ => Duplex::Unknown,
        },
        addresses: addresses
            .iter()
            .filter(|(index, _)| *index == record.index)
            .map(|(_, address)| *address)
            .collect(),
    }
}

/// `IFA_LOCAL` is the own address on point-to-point links, where
/// `IFA_ADDRESS` names the peer; elsewhere both are equal.
pub(crate) fn address_record(payload: &[u8]) -> Option<(u32, InterfaceAddress)> {
    let message = AddressMessage::parse(payload).ok()?;
    let mut local = None;
    let mut address = None;
    for attribute in message.attributes {
        match attribute {
            AddressAttribute::Local(ip) => local = Some(ip),
            AddressAttribute::Address(ip) => address = Some(ip),
            _ => {}
        }
    }
    Some((
        message.header.index,
        InterfaceAddress {
            address: local.or(address)?,
            prefix_length: message.header.prefix_len,
        },
    ))
}

/// A default route of the main table: `(output interface, gateway)`.
pub(crate) fn default_route(payload: &[u8]) -> Option<(u32, IpAddr)> {
    let message = RouteMessage::parse(payload).ok()?;
    if message.header.destination_prefix_length != 0 {
        return None;
    }
    let mut table = u32::from(message.header.table);
    let mut gateway = None;
    let mut output = None;
    for attribute in message.attributes {
        match attribute {
            RouteAttribute::Table(id) => table = id,
            RouteAttribute::Gateway(RouteAddress::Inet(ip)) => gateway = Some(IpAddr::V4(ip)),
            RouteAttribute::Gateway(RouteAddress::Inet6(ip)) => gateway = Some(IpAddr::V6(ip)),
            RouteAttribute::Oif(index) => output = Some(index),
            _ => {}
        }
    }
    if table != MAIN_ROUTE_TABLE {
        return None;
    }
    Some((output?, gateway?))
}

fn format_mac(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.iter().all(|byte| *byte == 0) {
        return None;
    }
    let mut text = String::with_capacity(bytes.len() * 3);
    for (position, byte) in bytes.iter().enumerate() {
        if position > 0 {
            text.push(':');
        }
        let _ = write!(text, "{byte:02x}");
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_mac_addresses() {
        assert_eq!(
            format_mac(&[0xdc, 0xf7, 0x56, 0x03, 0x1d, 0x11]).as_deref(),
            Some("dc:f7:56:03:1d:11")
        );
        assert_eq!(format_mac(&[0; 6]), None);
        assert_eq!(format_mac(&[]), None);
    }

    #[test]
    fn maps_operational_states() {
        assert_eq!(OperState::from_raw(6), OperState::Up);
        assert_eq!(OperState::from_raw(2), OperState::Down);
        assert_eq!(OperState::from_sysfs("dormant"), OperState::Dormant);
    }

    #[test]
    fn lists_interfaces_from_sysfs() {
        let records = sysfs_records();
        let Some(loopback) = records.iter().find(|record| record.name == "lo") else {
            return;
        };
        let info = describe(loopback, &[]);
        assert_eq!(info.kind, InterfaceKind::Loopback);
        assert_eq!(info.index, 1);
    }
}
