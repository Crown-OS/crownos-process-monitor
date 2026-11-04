//! The Network page over a seeded tick: Wi-Fi, a VPN, loopback and two listening sockets.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use crowntest::{Role, role, text};
use monitor_core::{BytesPerSecond, Coverage, Percent, Uid};
use monitor_engine::Snapshot;
use monitor_engine::io::{
    BindScope, Connection, DefaultRoute, DnsServers, DnsSource, Duplex, InterfaceAddress,
    InterfaceCounters, InterfaceInfo, InterfaceKind, InterfaceRates, ListeningPort,
    NetworkInterface, NetworkSnapshot, OperState, Protocol, TcpState, WifiBand, WifiInfo,
    WifiSecurity,
};

use super::page;
use crate::process_table::test_support::{live_with, window};

/// Whether at least one node matches; text often repeats (a panel and a row share a title).
fn shows(app: &mut crowntest::TestApp, query: crowntest::Query) -> bool {
    app.count(query) > 0
}

fn interface(index: u32, name: &str, kind: InterfaceKind) -> NetworkInterface {
    NetworkInterface {
        info: Arc::new(InterfaceInfo {
            index,
            name: name.into(),
            kind,
            link_kind: None,
            mac: Some("aa:bb:cc:dd:ee:ff".into()),
            mtu: Some(1500),
            speed_mbps: None,
            duplex: Duplex::Unknown,
            addresses: vec![InterfaceAddress {
                address: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
                prefix_length: 24,
            }],
        }),
        state: OperState::Up,
        counters: InterfaceCounters::default(),
        rates: InterfaceRates {
            receive: BytesPerSecond::new(3e5),
            ..InterfaceRates::default()
        },
    }
}

fn listening(port: u16, address: Ipv4Addr) -> ListeningPort {
    ListeningPort {
        protocol: Protocol::Tcp,
        address: SocketAddr::new(IpAddr::V4(address), port),
        scope: BindScope::of(IpAddr::V4(address)),
        owner: None,
        uid: Uid::new(1000),
        inode: u64::from(port),
    }
}

fn seeded_network() -> NetworkSnapshot {
    NetworkSnapshot {
        interfaces: vec![
            interface(1, "lo", InterfaceKind::Loopback),
            interface(2, "wlo1", InterfaceKind::Wifi),
            interface(3, "tailscale0", InterfaceKind::Vpn),
        ],
        default_routes: Arc::from(vec![DefaultRoute {
            gateway: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
            interface_index: 2,
            interface: Some("wlo1".into()),
        }]),
        dns: DnsServers {
            servers: Arc::from(vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))]),
            source: DnsSource::Resolved,
        },
        wifi: Arc::from(vec![WifiInfo {
            interface: "wlo1".into(),
            ssid: Some("Crown Home".into()),
            strength: Some(Percent::new(72.0)),
            signal_dbm: Some(-54),
            band: WifiBand::FiveGhz,
            channel: Some(36),
            link_rate_mbps: Some(866),
            security: WifiSecurity::Wpa3Personal,
            ..WifiInfo::default()
        }]),
        listening: vec![
            listening(631, Ipv4Addr::LOCALHOST),
            listening(22, Ipv4Addr::UNSPECIFIED),
        ],
        connections: vec![Connection {
            protocol: Protocol::Tcp,
            local: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)), 50_000),
            remote: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 9)), 443),
            state: TcpState::Established,
            receive_queue: 0,
            send_queue: 0,
            owner: None,
            uid: Uid::new(1000),
            inode: 9,
        }],
        process_traffic_coverage: Coverage::TcpOnly,
        ..NetworkSnapshot::default()
    }
}

#[test]
fn network_renders_its_sections_from_the_tick() {
    let tick = Snapshot {
        network: seeded_network(),
        ..Snapshot::default()
    };
    let mut live = live_with(tick.clone());
    live.network.push(&tick);
    let mut app = window(live, page);
    app.settle();
    for title in [
        "Traffic",
        "Received and sent",
        "Interfaces",
        "Routes and DNS",
        "Wi-Fi",
        "Listening ports",
        "Connections",
        "Apps",
    ] {
        assert!(shows(&mut app, text(title)), "no {title} panel");
    }
    assert!(app.exists(role(Role::Group).name("wlo1")));
    assert!(app.exists(role(Role::Group).name("tailscale0")));
    assert!(
        !app.exists(role(Role::Group).name("lo")),
        "loopback is not listed"
    );
    assert!(
        shows(&mut app, text("History is not available")),
        "interface cards list their history totals"
    );
    assert!(
        shows(&mut app, text("Counted separately")),
        "tunnels stay out of the totals"
    );
    assert!(app.exists(role(Role::Group).name("Crown Home")));
    assert!(shows(&mut app, text("-54 dBm")));
    assert!(shows(&mut app, text("192.168.1.1 via wlo1")));
    assert!(app.exists(role(Role::ListItem).name("TCP 631")));
    assert!(shows(&mut app, text("Loopback only")));
    assert!(shows(&mut app, text("Exposed on all interfaces")));
    assert!(app.exists(role(Role::ListItem).text("192.168.1.9:443")));
    assert!(
        shows(&mut app, text("TCP only")),
        "per-app traffic is TCP only without the helper"
    );
    assert!(app.exists(role(Role::Button).name("wlo1 · Wi-Fi")));
}

#[test]
fn interface_chips_toggle_the_chart_sum() {
    let tick = Snapshot {
        network: seeded_network(),
        ..Snapshot::default()
    };
    let mut live = live_with(tick.clone());
    live.network.push(&tick);
    let mut app = window(live, page);
    app.settle();
    let chip = role(Role::Button).name("tailscale0 · VPN");
    assert_eq!(app.find(chip.clone()).value.as_deref(), Some("hidden"));
    app.click(chip.clone());
    app.settle();
    assert_eq!(app.find(chip).value.as_deref(), Some("shown"));
}
