//! Per-interface traffic counters: `IFLA_STATS64` from the per-tick
//! `RTM_GETLINK` dump, read without building `LinkMessage`s, or sysfs.

use std::path::Path;

use monitor_core::util::sysfs;
use monitor_core::{Bytes, BytesPerSecond};
use netlink_packet_core::NlasIterator;
use netlink_packet_route::link::{LinkHeader, Stats64};

use crate::rate::per_second;

const LINK_HEADER_LEN: usize = 16;
const IFLA_OPERSTATE: u16 = 16;
const IFLA_STATS64: u16 = 23;
const SYS_CLASS_NET: &str = "/sys/class/net";

/// Cumulative counters since the interface appeared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InterfaceCounters {
    pub receive_bytes: Bytes,
    pub transmit_bytes: Bytes,
    pub receive_packets: u64,
    pub transmit_packets: u64,
    pub receive_errors: u64,
    pub transmit_errors: u64,
    pub receive_drops: u64,
    pub transmit_drops: u64,
}

/// Rates over the last interval.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InterfaceRates {
    pub receive: BytesPerSecond,
    pub transmit: BytesPerSecond,
    pub receive_packets: f32,
    pub transmit_packets: f32,
    /// Receive plus transmit errors per second.
    pub errors: f32,
    pub drops: f32,
}

impl InterfaceRates {
    pub(crate) fn between(
        earlier: &InterfaceCounters,
        later: &InterfaceCounters,
        seconds: f64,
    ) -> Self {
        let rate = |before: u64, after: u64| per_second(before, after, seconds);
        Self {
            receive: BytesPerSecond::between(
                earlier.receive_bytes.get(),
                later.receive_bytes.get(),
                seconds,
            ),
            transmit: BytesPerSecond::between(
                earlier.transmit_bytes.get(),
                later.transmit_bytes.get(),
                seconds,
            ),
            receive_packets: rate(earlier.receive_packets, later.receive_packets),
            transmit_packets: rate(earlier.transmit_packets, later.transmit_packets),
            errors: rate(earlier.receive_errors, later.receive_errors)
                + rate(earlier.transmit_errors, later.transmit_errors),
            drops: rate(earlier.receive_drops, later.receive_drops)
                + rate(earlier.transmit_drops, later.transmit_drops),
        }
    }

    pub(crate) fn accumulate(&mut self, other: &Self) {
        self.receive += other.receive;
        self.transmit += other.transmit;
        self.receive_packets += other.receive_packets;
        self.transmit_packets += other.transmit_packets;
        self.errors += other.errors;
        self.drops += other.drops;
    }
}

impl From<&Stats64> for InterfaceCounters {
    fn from(stats: &Stats64) -> Self {
        Self {
            receive_bytes: Bytes::new(stats.rx_bytes),
            transmit_bytes: Bytes::new(stats.tx_bytes),
            receive_packets: stats.rx_packets,
            transmit_packets: stats.tx_packets,
            receive_errors: stats.rx_errors,
            transmit_errors: stats.tx_errors,
            receive_drops: stats.rx_dropped,
            transmit_drops: stats.tx_dropped,
        }
    }
}

/// One `RTM_NEWLINK` payload of the counters dump: interface index,
/// operational state and counters. Allocation-free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LinkReading {
    pub index: u32,
    pub operstate: Option<u8>,
    pub counters: Option<InterfaceCounters>,
}

pub(crate) fn parse_link_reading(payload: &[u8]) -> Option<LinkReading> {
    let header = LinkHeader::parse(payload).ok()?;
    let mut reading = LinkReading {
        index: header.index,
        operstate: None,
        counters: None,
    };
    for attribute in NlasIterator::new(payload.get(LINK_HEADER_LEN..)?).flatten() {
        match attribute.kind() {
            IFLA_OPERSTATE => reading.operstate = attribute.value().first().copied(),
            IFLA_STATS64 => {
                reading.counters = Stats64::parse(attribute.value())
                    .ok()
                    .map(|stats| InterfaceCounters::from(&stats));
            }
            _ => {}
        }
    }
    Some(reading)
}

/// Fallback when rtnetlink is unavailable.
pub(crate) fn sysfs_counters(interface: &str) -> Option<InterfaceCounters> {
    let statistics = Path::new(SYS_CLASS_NET).join(interface).join("statistics");
    let read = |name: &str| sysfs::read_u64(statistics.join(name)).ok();
    Some(InterfaceCounters {
        receive_bytes: Bytes::new(read("rx_bytes")?),
        transmit_bytes: Bytes::new(read("tx_bytes")?),
        receive_packets: read("rx_packets").unwrap_or(0),
        transmit_packets: read("tx_packets").unwrap_or(0),
        receive_errors: read("rx_errors").unwrap_or(0),
        transmit_errors: read("tx_errors").unwrap_or(0),
        receive_drops: read("rx_dropped").unwrap_or(0),
        transmit_drops: read("tx_dropped").unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_rates_and_ignores_resets() {
        let earlier = InterfaceCounters {
            receive_bytes: Bytes::new(1000),
            transmit_bytes: Bytes::new(500),
            receive_packets: 10,
            receive_errors: 1,
            ..InterfaceCounters::default()
        };
        let later = InterfaceCounters {
            receive_bytes: Bytes::new(3000),
            transmit_bytes: Bytes::new(100),
            receive_packets: 30,
            receive_errors: 3,
            ..InterfaceCounters::default()
        };
        let rates = InterfaceRates::between(&earlier, &later, 2.0);
        assert_eq!(rates.receive.get(), 1000.0);
        assert_eq!(rates.transmit, BytesPerSecond::ZERO);
        assert_eq!(rates.receive_packets, 10.0);
        assert_eq!(rates.errors, 1.0);
    }

    #[test]
    fn parses_a_link_payload() {
        let mut payload = vec![0_u8; LINK_HEADER_LEN];
        payload[4..8].copy_from_slice(&3_u32.to_ne_bytes());
        payload.extend_from_slice(&5_u16.to_ne_bytes());
        payload.extend_from_slice(&IFLA_OPERSTATE.to_ne_bytes());
        payload.extend_from_slice(&[6, 0, 0, 0]);
        let stats_length = 4 + 8 * 4;
        payload.extend_from_slice(&(stats_length as u16).to_ne_bytes());
        payload.extend_from_slice(&IFLA_STATS64.to_ne_bytes());
        for value in [11_u64, 22, 3300, 4400] {
            payload.extend_from_slice(&value.to_ne_bytes());
        }
        let Some(reading) = parse_link_reading(&payload) else {
            panic!("payload must parse");
        };
        assert_eq!(reading.index, 3);
        assert_eq!(reading.operstate, Some(6));
        let counters = reading.counters.unwrap_or_default();
        assert_eq!(counters.receive_packets, 11);
        assert_eq!(counters.transmit_packets, 22);
        assert_eq!(counters.receive_bytes.get(), 3300);
        assert_eq!(counters.transmit_bytes.get(), 4400);
    }

    #[test]
    fn reads_loopback_from_sysfs() {
        assert!(sysfs_counters("lo").is_some());
        assert!(sysfs_counters("does-not-exist").is_none());
    }
}
