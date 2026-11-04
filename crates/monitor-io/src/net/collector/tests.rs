use monitor_core::util::procfs::ProcRoot;
use monitor_core::{Coverage, FdIndex, FdScope, Pid};

use super::*;
use crate::RateStatus;
use crate::net::interfaces::OperState;
use crate::net::kind::InterfaceKind;
use crate::net::sockets::{BindScope, Protocol};

fn interest_on_network() -> Interest {
    Interest {
        page: Page::Network,
        ..Interest::default()
    }
}

#[test]
fn classifies_live_interfaces_and_sees_own_listener() {
    let Ok(listener) = std::net::TcpListener::bind("0.0.0.0:0") else {
        return;
    };
    let Ok(port) = listener.local_addr().map(|address| address.port()) else {
        return;
    };
    let own = Pid::new(std::process::id());
    let mut fds = FdIndex::default();
    if let Ok(root) = ProcRoot::open() {
        fds.refresh(
            &root,
            &[own],
            FdScope {
                sockets: true,
                drm: false,
            },
        );
    }
    let interest = interest_on_network();
    assert!(crate::wants_socket_owners(&interest));
    let mut collector = NetworkCollector::new();
    let mut output = NetworkSnapshot::default();
    let mut earlier = Tick::now();
    for _ in 0..2 {
        std::thread::sleep(Duration::from_millis(50));
        let tick = Tick::now();
        let context = SampleContext {
            tick,
            elapsed: tick.since(&earlier),
            interest: &interest,
            fds: &fds,
        };
        assert!(collector.sample(&context, &mut output).is_ok());
        earlier = tick;
    }
    assert_eq!(output.status, RateStatus::Measured);
    let kind_of = |name: &str| {
        output
            .interfaces
            .iter()
            .find(|interface| &*interface.info.name == name)
            .map(|interface| interface.info.kind)
    };
    assert_eq!(kind_of("lo"), Some(InterfaceKind::Loopback));
    for (name, expected) in [
        ("wlo1", &[InterfaceKind::Wifi][..]),
        ("tailscale0", &[InterfaceKind::Vpn]),
        (
            "waydroid0",
            &[InterfaceKind::Bridge, InterfaceKind::Virtual],
        ),
        ("enp2s0", &[InterfaceKind::Ethernet]),
    ] {
        if let Some(kind) = kind_of(name) {
            assert!(expected.contains(&kind), "{name} classified {kind:?}");
        }
    }
    if let Some(wifi) = output
        .interfaces
        .iter()
        .find(|interface| &*interface.info.name == "wlo1")
    {
        assert!(wifi.info.mac.is_some());
        assert!(!wifi.info.addresses.is_empty());
        assert!(wifi.counters.receive_bytes.get() > 0);
        if wifi.state == OperState::Up {
            assert!(
                output
                    .default_routes
                    .iter()
                    .any(|route| route.interface.as_deref() == Some("wlo1"))
            );
        }
    }
    let ours = output
        .listening
        .iter()
        .find(|entry| entry.protocol == Protocol::Tcp && entry.address.port() == port);
    assert!(
        ours.is_some_and(
            |entry| entry.owner == Some(own) && entry.scope == BindScope::AllInterfaces
        )
    );
    assert_eq!(output.process_traffic_coverage, Coverage::TcpOnly);
}

#[test]
fn skips_socket_lists_off_page() {
    let interest = Interest::default();
    assert!(!crate::wants_socket_owners(&interest));
    let fds = FdIndex::default();
    let mut collector = NetworkCollector::new();
    let mut output = NetworkSnapshot::default();
    let tick = Tick::now();
    let context = SampleContext {
        tick,
        elapsed: tick.since(&tick),
        interest: &interest,
        fds: &fds,
    };
    assert!(collector.sample(&context, &mut output).is_ok());
    assert!(output.listening.is_empty() && output.connections.is_empty());
    assert_eq!(output.process_traffic_coverage, Coverage::Unavailable);
    assert!(
        collector.socket_owners_due(&interest, &tick),
        "history asks for traffic before the first pass"
    );
    assert_eq!(output.status, RateStatus::Warmup);
}

#[test]
fn lists_the_inspected_process_connections_off_page() {
    let Ok(listener) = std::net::TcpListener::bind("127.0.0.1:0") else {
        return;
    };
    let Ok(address) = listener.local_addr() else {
        return;
    };
    let Ok(_client) = std::net::TcpStream::connect(address) else {
        return;
    };
    let own = Pid::new(std::process::id());
    let Some(start_ticks) = std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(')')?
                .1
                .split_whitespace()
                .nth(19)?
                .parse()
                .ok()
        })
    else {
        return;
    };
    let mut fds = FdIndex::default();
    if let Ok(root) = ProcRoot::open() {
        fds.refresh(
            &root,
            &[own],
            FdScope {
                sockets: true,
                drm: false,
            },
        );
    }
    let interest = Interest {
        inspected: Some(monitor_core::ProcessKey {
            pid: own,
            start_ticks,
        }),
        ..Interest::default()
    };
    let mut collector = NetworkCollector::new();
    let mut output = NetworkSnapshot::default();
    let tick = Tick::now();
    let context = SampleContext {
        tick,
        elapsed: tick.since(&tick),
        interest: &interest,
        fds: &fds,
    };
    assert!(collector.sample(&context, &mut output).is_ok());
    assert!(
        output
            .connections
            .iter()
            .any(|connection| connection.remote == address)
    );
    assert!(
        output
            .connections
            .iter()
            .all(|connection| connection.owner == Some(own))
    );
}
