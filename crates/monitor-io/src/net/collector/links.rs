use std::sync::Arc;

use monitor_core::{Bytes, Elapsed};

use super::{NetworkCollector, RTM_NEWADDR, RTM_NEWLINK, RTM_NEWROUTE};
use crate::RateStatus;
use crate::net::counters::{self, InterfaceCounters, InterfaceRates};
use crate::net::interfaces::{self, DefaultRoute, InterfaceInfo, OperState};
use crate::net::{NetworkInterface, NetworkSnapshot};

#[derive(Debug)]
pub(super) struct TrackedInterface {
    info: Arc<InterfaceInfo>,
    state: OperState,
    previous: Option<InterfaceCounters>,
    current: Option<InterfaceCounters>,
}

impl NetworkCollector {
    pub(super) fn refresh_info(&mut self) {
        let mut records = Vec::new();
        let mut addresses = Vec::new();
        let mut gateways = Vec::new();
        if let Some(route) = &mut self.route {
            let listed = route.dump(&mut self.requests.links, |kind, payload| {
                if kind == RTM_NEWLINK
                    && let Some(record) = interfaces::link_record(payload)
                {
                    records.push(record);
                }
            });
            if listed.is_err() {
                records.clear();
            }
            let _ = route.dump(&mut self.requests.addresses, |kind, payload| {
                if kind == RTM_NEWADDR
                    && let Some(address) = interfaces::address_record(payload)
                {
                    addresses.push(address);
                }
            });
            let _ = route.dump(&mut self.requests.routes, |kind, payload| {
                if kind == RTM_NEWROUTE
                    && let Some(gateway) = interfaces::default_route(payload)
                {
                    gateways.push(gateway);
                }
            });
        }
        if records.is_empty() {
            records = interfaces::sysfs_records();
        }
        records.sort_by_key(|record| record.index);
        let previous = std::mem::take(&mut self.interfaces);
        self.interfaces = records
            .iter()
            .map(|record| {
                let kept = previous.iter().find(|known| {
                    known.info.index == record.index && *known.info.name == *record.name
                });
                TrackedInterface {
                    info: Arc::new(interfaces::describe(record, &addresses)),
                    state: record.operstate,
                    previous: kept.and_then(|known| known.previous),
                    current: None,
                }
            })
            .collect();
        self.routes = gateways
            .into_iter()
            .map(|(index, gateway)| DefaultRoute {
                gateway,
                interface_index: index,
                interface: self
                    .interfaces
                    .iter()
                    .find(|tracked| tracked.info.index == index)
                    .map(|tracked| Arc::clone(&tracked.info.name)),
            })
            .collect();
    }

    pub(super) fn read_counters(&mut self) {
        for interface in &mut self.interfaces {
            interface.current = None;
        }
        let mut unknown_interface = false;
        let interfaces = &mut self.interfaces;
        let dumped = self.route.as_mut().is_some_and(|route| {
            route
                .dump(&mut self.requests.links, |kind, payload| {
                    if kind != RTM_NEWLINK {
                        return;
                    }
                    let Some(reading) = counters::parse_link_reading(payload) else {
                        return;
                    };
                    let Some(interface) = interfaces
                        .iter_mut()
                        .find(|known| known.info.index == reading.index)
                    else {
                        unknown_interface = true;
                        return;
                    };
                    interface.current = reading.counters;
                    if let Some(state) = reading.operstate {
                        interface.state = OperState::from_raw(state);
                    }
                })
                .is_ok()
        });
        if !dumped {
            for interface in &mut self.interfaces {
                interface.current = counters::sysfs_counters(&interface.info.name);
            }
        }
        self.refresh_needed |= unknown_interface;
    }

    pub(super) fn fill_interfaces(&mut self, elapsed: Elapsed, output: &mut NetworkSnapshot) {
        let Elapsed { seconds, suspended } = elapsed;
        output.interfaces.clear();
        output.totals = InterfaceRates::default();
        output.vpn_totals = InterfaceRates::default();
        let mut receive_total = Bytes::ZERO;
        let mut transmit_total = Bytes::ZERO;
        for interface in &mut self.interfaces {
            let Some(current) = interface.current else {
                continue;
            };
            let rates = match interface.previous {
                Some(previous) if !suspended => {
                    InterfaceRates::between(&previous, &current, seconds)
                }
                _ => InterfaceRates::default(),
            };
            interface.previous = Some(current);
            if interface.info.kind.counts_in_totals() {
                output.totals.accumulate(&rates);
                receive_total += current.receive_bytes;
                transmit_total += current.transmit_bytes;
            } else if interface.info.kind.is_tunnel() {
                output.vpn_totals.accumulate(&rates);
            }
            output.interfaces.push(NetworkInterface {
                info: Arc::clone(&interface.info),
                state: interface.state,
                counters: current,
                rates,
            });
        }
        output.receive_total = receive_total;
        output.transmit_total = transmit_total;
        output.status = RateStatus::from_elapsed(self.had_counters, suspended);
        self.had_counters = !output.interfaces.is_empty();
    }
}
