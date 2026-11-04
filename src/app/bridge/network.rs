//! Network histories: the counted totals, one pair per interface and the sum of the interfaces
//! the chart's toggles show. Lists (interfaces, sockets, Wi-Fi) come from the latest snapshot.

use std::sync::Arc;

use monitor_engine::Snapshot;
use monitor_engine::io::{InterfaceKind, NetworkInterface};

use crate::app::live::{HISTORY_CAPACITY, push_sample};

/// Rates are bytes per second.
#[derive(Debug, Default)]
pub struct NetworkReadings {
    pub download: Vec<f32>,
    pub upload: Vec<f32>,
    /// Oldest day first.
    pub daily_bytes: Vec<f32>,
    /// Weekday of `daily_bytes[0]`, Monday = 0.
    pub daily_first_weekday: usize,
    pub wifi_signal: f32,
    pub wifi_interface: String,
    /// Every interface but loopback, in the order they appeared.
    pub interfaces: Vec<InterfaceHistory>,
    /// The sum of the interfaces whose toggle is on.
    pub shown_download: Vec<f32>,
    pub shown_upload: Vec<f32>,
}

#[derive(Debug)]
pub struct InterfaceHistory {
    pub name: Arc<str>,
    pub kind: InterfaceKind,
    /// Starts on for the interfaces the totals count.
    pub shown: bool,
    pub download: Vec<f32>,
    pub upload: Vec<f32>,
}

impl InterfaceHistory {
    fn new(interface: &NetworkInterface) -> Self {
        let info = &interface.info;
        Self {
            name: Arc::clone(&info.name),
            kind: info.kind,
            shown: info.kind.counts_in_totals(),
            download: vec![f32::NAN; HISTORY_CAPACITY],
            upload: vec![f32::NAN; HISTORY_CAPACITY],
        }
    }
}

impl NetworkReadings {
    pub fn prepare(&mut self) {
        for history in [
            &mut self.download,
            &mut self.upload,
            &mut self.shown_download,
            &mut self.shown_upload,
        ] {
            history.clear();
            history.resize(HISTORY_CAPACITY, f32::NAN);
        }
    }

    pub fn push(&mut self, snapshot: &Snapshot) {
        let network = &snapshot.network;
        push_sample(&mut self.download, network.totals.receive.get() as f32);
        push_sample(&mut self.upload, network.totals.transmit.get() as f32);
        self.follow_interfaces(&network.interfaces);
        for history in &mut self.interfaces {
            let rates = network
                .interfaces
                .iter()
                .find(|interface| interface.info.name == history.name)
                .map(|interface| interface.rates);
            push_sample(
                &mut history.download,
                rates.map_or(f32::NAN, |rates| rates.receive.get() as f32),
            );
            push_sample(
                &mut history.upload,
                rates.map_or(f32::NAN, |rates| rates.transmit.get() as f32),
            );
        }
        let (download, upload) = self.shown_at(HISTORY_CAPACITY - 1);
        push_sample(&mut self.shown_download, download);
        push_sample(&mut self.shown_upload, upload);
    }

    pub fn push_gap(&mut self) {
        for history in [
            &mut self.download,
            &mut self.upload,
            &mut self.shown_download,
            &mut self.shown_upload,
        ] {
            push_sample(history, f32::NAN);
        }
        for history in &mut self.interfaces {
            push_sample(&mut history.download, f32::NAN);
            push_sample(&mut history.upload, f32::NAN);
        }
    }

    pub fn refresh(&mut self, snapshot: &Snapshot) {
        let wifi = snapshot.network.wifi.first();
        self.wifi_signal = wifi
            .and_then(|wifi| wifi.strength)
            .map_or(0.0, |strength| strength.get() / 100.0);
        self.wifi_interface.clear();
        if let Some(wifi) = wifi {
            self.wifi_interface.push_str(&wifi.interface);
        }
    }

    /// Flips one interface's toggle and redraws the shown sum over the whole window.
    pub fn toggle_interface(&mut self, name: &str) {
        let Some(history) = self
            .interfaces
            .iter_mut()
            .find(|history| &*history.name == name)
        else {
            return;
        };
        history.shown = !history.shown;
        for index in 0..self.shown_download.len().min(HISTORY_CAPACITY) {
            let (download, upload) = self.shown_at(index);
            self.shown_download[index] = download;
            self.shown_upload[index] = upload;
        }
    }

    /// Drops histories of vanished interfaces and starts empty ones for new interfaces.
    fn follow_interfaces(&mut self, interfaces: &[NetworkInterface]) {
        self.interfaces.retain(|history| {
            interfaces
                .iter()
                .any(|interface| interface.info.name == history.name)
        });
        for interface in interfaces {
            let known = self
                .interfaces
                .iter()
                .any(|history| history.name == interface.info.name);
            if !known && interface.info.kind != InterfaceKind::Loopback {
                self.interfaces.push(InterfaceHistory::new(interface));
            }
        }
    }

    /// The shown interfaces' sum at one sample; NaN when none of them has a value there.
    fn shown_at(&self, index: usize) -> (f32, f32) {
        let mut sums = (f32::NAN, f32::NAN);
        for history in self.interfaces.iter().filter(|history| history.shown) {
            add_finite(&mut sums.0, history.download.get(index).copied());
            add_finite(&mut sums.1, history.upload.get(index).copied());
        }
        sums
    }
}

fn add_finite(sum: &mut f32, value: Option<f32>) {
    if let Some(value) = value.filter(|value| value.is_finite()) {
        *sum = if sum.is_finite() { *sum + value } else { value };
    }
}

#[cfg(test)]
mod tests {
    use monitor_core::BytesPerSecond;
    use monitor_engine::io::{
        InterfaceCounters, InterfaceInfo, InterfaceRates, NetworkSnapshot, OperState,
    };

    use super::*;

    fn interface(name: &str, kind: InterfaceKind, receive: f64) -> NetworkInterface {
        NetworkInterface {
            info: Arc::new(InterfaceInfo {
                index: 1,
                name: name.into(),
                kind,
                link_kind: None,
                mac: None,
                mtu: None,
                speed_mbps: None,
                duplex: Default::default(),
                addresses: Vec::new(),
            }),
            state: OperState::Up,
            counters: InterfaceCounters::default(),
            rates: InterfaceRates {
                receive: BytesPerSecond::new(receive),
                ..InterfaceRates::default()
            },
        }
    }

    fn tick(interfaces: Vec<NetworkInterface>) -> Snapshot {
        Snapshot {
            network: NetworkSnapshot {
                interfaces,
                ..NetworkSnapshot::default()
            },
            ..Snapshot::default()
        }
    }

    #[test]
    fn toggles_sum_only_the_shown_interfaces() {
        let mut readings = NetworkReadings::default();
        readings.prepare();
        readings.push(&tick(vec![
            interface("lo", InterfaceKind::Loopback, 9.0),
            interface("wlo1", InterfaceKind::Wifi, 100.0),
            interface("tailscale0", InterfaceKind::Vpn, 40.0),
        ]));
        assert_eq!(readings.interfaces.len(), 2, "loopback is never charted");
        assert_eq!(readings.shown_download.last().copied(), Some(100.0));
        readings.toggle_interface("tailscale0");
        assert_eq!(readings.shown_download.last().copied(), Some(140.0));
        readings.toggle_interface("wlo1");
        assert_eq!(readings.shown_download.last().copied(), Some(40.0));
        readings.push(&tick(vec![interface("wlo1", InterfaceKind::Wifi, 5.0)]));
        assert_eq!(
            readings.interfaces.len(),
            1,
            "vanished interfaces are dropped"
        );
        assert!(
            readings
                .shown_download
                .last()
                .is_some_and(|value| value.is_nan()),
            "the only shown interface is gone"
        );
    }
}
