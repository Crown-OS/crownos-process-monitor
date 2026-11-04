//! Network interfaces, routes, DNS, Wi-Fi and sockets.

mod collector;
mod counters;
mod dns;
mod interfaces;
mod kind;
mod services;
mod sockets;
mod wifi;

use std::sync::Arc;

use monitor_core::{Bytes, Coverage, Interest, Page};

pub use collector::{HISTORY_TRAFFIC_INTERVAL, MINIMIZED_TRAFFIC_INTERVAL, NetworkCollector};
pub use counters::{InterfaceCounters, InterfaceRates};
pub use dns::{DnsServers, DnsSource};
pub use interfaces::{DefaultRoute, Duplex, InterfaceAddress, InterfaceInfo, OperState};
pub use kind::InterfaceKind;
pub use sockets::{BindScope, Connection, ListeningPort, ProcessTraffic, Protocol, TcpState};
pub use wifi::{WifiBand, WifiInfo, WifiSecurity, WifiSource};

use crate::RateStatus;

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkInterface {
    pub info: Arc<InterfaceInfo>,
    pub state: OperState,
    pub counters: InterfaceCounters,
    pub rates: InterfaceRates,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetworkSnapshot {
    pub status: RateStatus,
    pub interfaces: Vec<NetworkInterface>,
    /// Interfaces whose kind [`InterfaceKind::counts_in_totals`]: physical
    /// uplinks only, so tunnel and container traffic is not counted twice.
    pub totals: InterfaceRates,
    /// VPN and tunnel interfaces, shown separately.
    pub vpn_totals: InterfaceRates,
    /// Cumulative bytes of the counted interfaces.
    pub receive_total: Bytes,
    pub transmit_total: Bytes,
    pub default_routes: Arc<[DefaultRoute]>,
    pub dns: DnsServers,
    /// Refreshed every 5 s while the Network page is visible.
    pub wifi: Arc<[WifiInfo]>,
    /// Filled while the Network page is visible or a process is inspected.
    pub listening: Vec<ListeningPort>,
    /// Every connection while the Network page is visible; only the
    /// inspected process's while it is not.
    pub connections: Vec<Connection>,
    /// Per-process TCP throughput, measured when the engine refreshed the
    /// [`monitor_core::FdIndex`] with the sockets scope (every tick on the
    /// Network page, every 5 s otherwise) and republished in between.
    pub process_traffic: Vec<ProcessTraffic>,
    /// Traffic passes so far: a new value means `process_traffic` was just
    /// measured rather than republished.
    pub traffic_passes: u64,
    pub process_traffic_coverage: Coverage,
}

/// Whether the engine should refresh the `FdIndex` sockets scope this tick:
/// socket owners are shown on the Network page and in the inspector.
pub fn wants_socket_owners(interest: &Interest) -> bool {
    interest.shows(Page::Network) || interest.inspected.is_some()
}
