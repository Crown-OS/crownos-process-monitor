//! Wi-Fi link details from NetworkManager, with the kernel's
//! `/proc/net/wireless` for signal level (and as the fallback without NM).

use std::sync::Arc;

use monitor_core::util::parse;
use monitor_core::{Megahertz, Percent};
use zbus::Connection;

use crate::util::dbus;

const NETWORK_MANAGER: &str = "org.freedesktop.NetworkManager";
const MANAGER_PATH: &str = "/org/freedesktop/NetworkManager";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
const DEVICE_TYPE_WIFI: u32 = 2;
const KILOBITS_PER_MEGABIT: u32 = 1000;
const AP_FLAGS_PRIVACY: u32 = 0x1;
const KEY_MGMT_PSK: u32 = 0x100;
const KEY_MGMT_802_1X: u32 = 0x200;
const KEY_MGMT_SAE: u32 = 0x400;
const KEY_MGMT_OWE: u32 = 0x800 | 0x1000;
const KEY_MGMT_SUITE_B: u32 = 0x2000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WifiBand {
    TwoPointFourGhz,
    FiveGhz,
    SixGhz,
    #[default]
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WifiSecurity {
    Open,
    Wep,
    WpaPersonal,
    Wpa2Personal,
    Wpa3Personal,
    Enterprise,
    /// Opportunistic Wireless Encryption ("Enhanced Open").
    Owe,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WifiSource {
    #[default]
    NetworkManager,
    /// `/proc/net/wireless` only: signal, no SSID.
    Kernel,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WifiInfo {
    pub interface: Arc<str>,
    pub ssid: Option<Arc<str>>,
    pub bssid: Option<Arc<str>>,
    /// NetworkManager's 0–100 quality.
    pub strength: Option<Percent>,
    pub signal_dbm: Option<i32>,
    pub frequency: Option<Megahertz>,
    pub band: WifiBand,
    pub channel: Option<u16>,
    pub link_rate_mbps: Option<u32>,
    pub max_rate_mbps: Option<u32>,
    pub security: WifiSecurity,
    pub source: WifiSource,
}

/// Maps a centre frequency to its band and IEEE channel number.
pub(crate) fn band_and_channel(mhz: u32) -> (WifiBand, Option<u16>) {
    let channel = |base: u32| u16::try_from((mhz - base) / 5).ok();
    match mhz {
        2484 => (WifiBand::TwoPointFourGhz, Some(14)),
        2412..=2472 => (WifiBand::TwoPointFourGhz, channel(2407)),
        5955..=7115 => (WifiBand::SixGhz, channel(5950)),
        5150..=5925 => (WifiBand::FiveGhz, channel(5000)),
        _ => (WifiBand::Other, None),
    }
}

pub(crate) fn security(flags: u32, wpa_flags: u32, rsn_flags: u32) -> WifiSecurity {
    let key_management = wpa_flags | rsn_flags;
    if key_management & (KEY_MGMT_802_1X | KEY_MGMT_SUITE_B) != 0 {
        WifiSecurity::Enterprise
    } else if rsn_flags & KEY_MGMT_SAE != 0 {
        WifiSecurity::Wpa3Personal
    } else if rsn_flags & KEY_MGMT_PSK != 0 {
        WifiSecurity::Wpa2Personal
    } else if wpa_flags & KEY_MGMT_PSK != 0 {
        WifiSecurity::WpaPersonal
    } else if key_management & KEY_MGMT_OWE != 0 {
        WifiSecurity::Owe
    } else if flags & AP_FLAGS_PRIVACY != 0 {
        WifiSecurity::Wep
    } else {
        WifiSecurity::Open
    }
}

/// One `/proc/net/wireless` row: interface, link quality and level (dBm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct KernelSignal<'a> {
    pub interface: &'a str,
    pub quality: f32,
    pub level_dbm: i32,
}

pub(crate) fn parse_wireless(text: &str) -> impl Iterator<Item = KernelSignal<'_>> {
    text.lines().skip(2).filter_map(|line| {
        let (interface, rest) = line.trim_start().split_once(':')?;
        let mut columns = rest.split_whitespace().skip(1);
        let mut number = || {
            columns
                .next()
                .and_then(|column| parse::parse_f64(column.trim_end_matches('.').as_bytes()))
        };
        let quality = number()? as f32;
        let level_dbm = number()? as i32;
        Some(KernelSignal {
            interface,
            quality,
            level_dbm,
        })
    })
}

/// Active Wi-Fi links as NetworkManager reports them; `None` without NM.
pub(crate) async fn from_network_manager(connection: &Connection) -> Option<Vec<WifiInfo>> {
    if !dbus::name_has_owner(connection, NETWORK_MANAGER).await {
        return None;
    }
    let reply = connection
        .call_method(
            Some(NETWORK_MANAGER),
            MANAGER_PATH,
            Some(NETWORK_MANAGER),
            "GetDevices",
            &(),
        )
        .await
        .ok()?;
    let devices: Vec<zbus::OwnedObjectPath> = reply.body().deserialize().ok()?;
    let mut links = Vec::new();
    for device in &devices {
        let Ok(kind) = dbus::get(
            connection,
            NETWORK_MANAGER,
            device.as_str(),
            DEVICE,
            "DeviceType",
        )
        .await
        else {
            continue;
        };
        if u32::try_from(&kind).ok() != Some(DEVICE_TYPE_WIFI) {
            continue;
        }
        if let Some(link) = wireless_device(connection, device.as_str()).await {
            links.push(link);
        }
    }
    Some(links)
}

async fn wireless_device(connection: &Connection, device: &str) -> Option<WifiInfo> {
    let interface = dbus::get(connection, NETWORK_MANAGER, device, DEVICE, "Interface")
        .await
        .ok()?;
    let interface: Arc<str> = Arc::from(<&str>::try_from(&interface).ok()?);
    let wireless = dbus::get_all(connection, NETWORK_MANAGER, device, WIRELESS)
        .await
        .ok()?;
    let mut info = WifiInfo {
        interface,
        link_rate_mbps: dbus::value::<u32>(&wireless, "Bitrate")
            .filter(|rate| *rate > 0)
            .map(|rate| rate / KILOBITS_PER_MEGABIT),
        ..WifiInfo::default()
    };
    let Some(access_point) = dbus::object_path(&wireless, "ActiveAccessPoint") else {
        return Some(info);
    };
    let point = dbus::get_all(connection, NETWORK_MANAGER, access_point, ACCESS_POINT)
        .await
        .ok()?;
    info.ssid = point
        .get("Ssid")
        .and_then(|ssid| dbus::bytes(ssid))
        .filter(|ssid| !ssid.is_empty())
        .map(|ssid| Arc::from(String::from_utf8_lossy(&ssid).as_ref()));
    info.bssid = dbus::string(&point, "HwAddress").map(Arc::from);
    info.strength =
        dbus::value::<u8>(&point, "Strength").map(|strength| Percent::clamped(f32::from(strength)));
    info.max_rate_mbps =
        dbus::value::<u32>(&point, "MaxBitrate").map(|rate| rate / KILOBITS_PER_MEGABIT);
    if let Some(mhz) = dbus::value::<u32>(&point, "Frequency").filter(|mhz| *mhz > 0) {
        info.frequency = Some(Megahertz::new(mhz));
        (info.band, info.channel) = band_and_channel(mhz);
    }
    info.security = security(
        dbus::value(&point, "Flags").unwrap_or(0),
        dbus::value(&point, "WpaFlags").unwrap_or(0),
        dbus::value(&point, "RsnFlags").unwrap_or(0),
    );
    Some(info)
}

/// Adds kernel signal levels to NM links, or builds kernel-only links
/// when NetworkManager is missing.
pub(crate) fn merge_kernel_signal(links: Option<Vec<WifiInfo>>, wireless: &str) -> Vec<WifiInfo> {
    match links {
        Some(mut links) => {
            for signal in parse_wireless(wireless) {
                if let Some(link) = links
                    .iter_mut()
                    .find(|link| &*link.interface == signal.interface)
                {
                    link.signal_dbm = Some(signal.level_dbm);
                }
            }
            links
        }
        None => parse_wireless(wireless)
            .map(|signal| WifiInfo {
                interface: Arc::from(signal.interface),
                signal_dbm: Some(signal.level_dbm),
                source: WifiSource::Kernel,
                ..WifiInfo::default()
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIRELESS: &str =
        "Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE
 face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22
  wlo1: 0000   39.  -71.  -256        0      0      0      4      1        0
";

    #[test]
    fn parses_proc_net_wireless() {
        let signals: Vec<_> = parse_wireless(WIRELESS).collect();
        assert_eq!(
            signals,
            [KernelSignal {
                interface: "wlo1",
                quality: 39.0,
                level_dbm: -71
            }]
        );
    }

    #[test]
    fn maps_frequencies_to_channels() {
        assert_eq!(band_and_channel(2412), (WifiBand::TwoPointFourGhz, Some(1)));
        assert_eq!(
            band_and_channel(2484),
            (WifiBand::TwoPointFourGhz, Some(14))
        );
        assert_eq!(band_and_channel(5180), (WifiBand::FiveGhz, Some(36)));
        assert_eq!(band_and_channel(5745), (WifiBand::FiveGhz, Some(149)));
        assert_eq!(band_and_channel(5955), (WifiBand::SixGhz, Some(1)));
        assert_eq!(band_and_channel(60_480), (WifiBand::Other, None));
    }

    #[test]
    fn derives_security_from_flags() {
        assert_eq!(security(0, 0, 0), WifiSecurity::Open);
        assert_eq!(security(1, 0, 0), WifiSecurity::Wep);
        assert_eq!(security(1, 0, 0x188), WifiSecurity::Wpa2Personal);
        assert_eq!(security(1, 0, 0x588), WifiSecurity::Wpa3Personal);
        assert_eq!(security(1, 0x144, 0), WifiSecurity::WpaPersonal);
        assert_eq!(security(1, 0, 0x288), WifiSecurity::Enterprise);
    }

    #[test]
    fn falls_back_to_kernel_signal() {
        let links = merge_kernel_signal(None, WIRELESS);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].source, WifiSource::Kernel);
        assert_eq!(links[0].signal_dbm, Some(-71));
        let named = WifiInfo {
            interface: Arc::from("wlo1"),
            ..WifiInfo::default()
        };
        let merged = merge_kernel_signal(Some(vec![named]), WIRELESS);
        assert_eq!(merged[0].signal_dbm, Some(-71));
        assert_eq!(merged[0].source, WifiSource::NetworkManager);
    }
}
