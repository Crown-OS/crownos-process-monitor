use std::sync::Arc;
use std::time::Duration;

use monitor_core::util::cadence::interval_elapsed;
use monitor_core::{Collector, Interest, Page, SampleContext, SourceResult, Tick};
use netlink_packet_route::RouteNetlinkMessage;
use netlink_packet_route::address::AddressMessage;
use netlink_packet_route::link::LinkMessage;
use netlink_packet_route::route::RouteMessage;
use netlink_sys::protocols::NETLINK_ROUTE;

use super::interfaces::DefaultRoute;
use super::services::NetworkServices;
use super::sockets::{ProcessTraffic, SocketMonitor};
use super::{NetworkSnapshot, wants_socket_owners};
use crate::util::netlink::{DumpSocket, EventSocket, dump_request};
use links::TrackedInterface;

mod links;
mod sockets;

const RTM_NEWLINK: u16 = 16;
const RTM_NEWADDR: u16 = 20;
const RTM_NEWROUTE: u16 = 24;
const RTMGRP_LINK: u32 = 0x1;
const RTMGRP_IPV4_IFADDR: u32 = 0x10;
const RTMGRP_IPV4_ROUTE: u32 = 0x40;
const RTMGRP_IPV6_IFADDR: u32 = 0x100;
const RTMGRP_IPV6_ROUTE: u32 = 0x400;
const EVENT_GROUPS: u32 =
    RTMGRP_LINK | RTMGRP_IPV4_IFADDR | RTMGRP_IPV4_ROUTE | RTMGRP_IPV6_IFADDR | RTMGRP_IPV6_ROUTE;
const SERVICES_INTERVAL: Duration = Duration::from_secs(5);
/// Without a multicast socket, interface facts are re-read this often.
const UNWATCHED_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
/// Per-process traffic is measured this often for history while no view
/// shows socket owners; ticks in between republish the last figures…
pub const HISTORY_TRAFFIC_INTERVAL: Duration = Duration::from_secs(5);
/// …and this often while the window is minimized. Byte counters are
/// cumulative, so only connections opened and closed in between are missed.
pub const MINIMIZED_TRAFFIC_INTERVAL: Duration = Duration::from_secs(15);

fn traffic_interval(interest: &Interest) -> Duration {
    if interest.is_visible() {
        HISTORY_TRAFFIC_INTERVAL
    } else {
        MINIMIZED_TRAFFIC_INTERVAL
    }
}

struct Requests {
    links: Vec<u8>,
    addresses: Vec<u8>,
    routes: Vec<u8>,
}

/// Samples interface counters with one rtnetlink dump per tick, refreshes
/// interface facts on rtnetlink events, and dumps sockets when they are
/// wanted. Wi-Fi and DNS come from a background D-Bus thread.
pub struct NetworkCollector {
    route: Option<DumpSocket>,
    events: Option<EventSocket>,
    requests: Requests,
    interfaces: Vec<TrackedInterface>,
    routes: Arc<[DefaultRoute]>,
    refresh_needed: bool,
    info_refreshed: Option<Duration>,
    had_counters: bool,
    sockets: Option<SocketMonitor>,
    last_traffic_pass: Option<Duration>,
    carried_traffic: Vec<ProcessTraffic>,
    services: NetworkServices,
    services_requested: Option<Duration>,
}

impl Default for NetworkCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkCollector {
    /// Opens the netlink sockets (falling back to sysfs when they fail) and
    /// starts the D-Bus worker thread.
    pub fn new() -> Self {
        Self {
            route: DumpSocket::open(NETLINK_ROUTE).ok(),
            events: EventSocket::open(NETLINK_ROUTE, EVENT_GROUPS).ok(),
            requests: Requests {
                links: dump_request(RouteNetlinkMessage::GetLink(LinkMessage::default())),
                addresses: dump_request(RouteNetlinkMessage::GetAddress(AddressMessage::default())),
                routes: dump_request(RouteNetlinkMessage::GetRoute(RouteMessage::default())),
            },
            interfaces: Vec::new(),
            routes: Arc::from([]),
            refresh_needed: true,
            info_refreshed: None,
            had_counters: false,
            sockets: SocketMonitor::open().ok(),
            last_traffic_pass: None,
            carried_traffic: Vec::new(),
            services: NetworkServices::spawn(),
            services_requested: None,
        }
    }

    /// Whether the engine should refresh the `FdIndex` sockets scope this
    /// tick: always while socket owners are shown, otherwise every
    /// [`HISTORY_TRAFFIC_INTERVAL`] ([`MINIMIZED_TRAFFIC_INTERVAL`] while
    /// minimized) so per-app history has network figures.
    pub fn socket_owners_due(&self, interest: &Interest, tick: &Tick) -> bool {
        wants_socket_owners(interest)
            || interval_elapsed(
                self.last_traffic_pass,
                tick.monotonic(),
                traffic_interval(interest),
            )
    }

    fn needs_refresh(&mut self, now: Duration) -> bool {
        let evented = self.events.as_mut().is_some_and(EventSocket::drain);
        let unwatched = self.events.is_none()
            && interval_elapsed(self.info_refreshed, now, UNWATCHED_REFRESH_INTERVAL);
        std::mem::take(&mut self.refresh_needed) || evented || unwatched
    }

    fn refresh_services(
        &mut self,
        context: &SampleContext<'_>,
        now: Duration,
        output: &mut NetworkSnapshot,
    ) {
        let wanted = self.services_requested.is_none()
            || (context.interest.shows(Page::Network)
                && interval_elapsed(self.services_requested, now, SERVICES_INTERVAL));
        if wanted {
            self.services_requested = Some(now);
            self.services.request_refresh();
        }
        if let Some(state) = self.services.latest() {
            output.wifi = Arc::clone(&state.wifi);
            output.dns = state.dns.clone();
        }
    }
}

impl Collector for NetworkCollector {
    type Output = NetworkSnapshot;

    fn sample(
        &mut self,
        context: &SampleContext<'_>,
        output: &mut NetworkSnapshot,
    ) -> SourceResult<()> {
        let now = context.tick.monotonic();
        if self.needs_refresh(now) {
            self.refresh_info();
            self.info_refreshed = Some(now);
            output.default_routes = Arc::clone(&self.routes);
        }
        self.read_counters();
        self.fill_interfaces(context.elapsed, output);
        self.sample_sockets(context, output);
        self.refresh_services(context, now, output);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
