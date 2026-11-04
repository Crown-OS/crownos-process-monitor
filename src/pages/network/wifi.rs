use std::sync::Arc;

use crownui::kit::icons;
use crownui::prelude::{
    AnyView, Color, Memo, Role, Runtime, Semantic, Styled, View, bind, hstack, keyed, text, vstack,
};
use monitor_engine::io::{WifiBand, WifiInfo, WifiSecurity};

use crate::app::AppState;
use crate::components::empty_state::empty_state;
use crate::components::fact_row::fact_row;
use crate::components::meter::signal_meter;
use crate::util::format::percent;
use crate::util::spacing;

const PERCENT: f32 = 100.0;

pub const fn band_name(band: WifiBand) -> &'static str {
    match band {
        WifiBand::TwoPointFourGhz => "2.4 GHz",
        WifiBand::FiveGhz => "5 GHz",
        WifiBand::SixGhz => "6 GHz",
        WifiBand::Other => "",
    }
}

pub const fn security_name(security: WifiSecurity) -> &'static str {
    match security {
        WifiSecurity::Open => "Open (unencrypted)",
        WifiSecurity::Wep => "WEP (insecure)",
        WifiSecurity::WpaPersonal => "WPA Personal",
        WifiSecurity::Wpa2Personal => "WPA2 Personal",
        WifiSecurity::Wpa3Personal => "WPA3 Personal",
        WifiSecurity::Enterprise => "Enterprise",
        WifiSecurity::Owe => "Enhanced Open",
        WifiSecurity::Unknown => "–",
    }
}

fn signal(wifi: &WifiInfo) -> String {
    match (wifi.signal_dbm, wifi.strength) {
        (Some(dbm), Some(quality)) => format!("{dbm} dBm · {} quality", percent(quality.get())),
        (Some(dbm), None) => format!("{dbm} dBm"),
        (None, Some(quality)) => format!("{} quality", percent(quality.get())),
        (None, None) => "–".to_owned(),
    }
}

fn radio(wifi: &WifiInfo) -> String {
    let frequency = wifi
        .frequency
        .map_or_else(String::new, |frequency| format!("{} MHz", frequency.get()));
    let channel = wifi
        .channel
        .map_or_else(String::new, |channel| format!("channel {channel}"));
    [band_name(wifi.band), frequency.as_str(), channel.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn link_rate(wifi: &WifiInfo) -> String {
    match (wifi.link_rate_mbps, wifi.max_rate_mbps) {
        (Some(rate), Some(max)) => format!("{rate} Mb/s (up to {max})"),
        (Some(rate), None) => format!("{rate} Mb/s"),
        (None, Some(max)) => format!("up to {max} Mb/s"),
        (None, None) => "–".to_owned(),
    }
}

fn wifi_card(state: AppState, index: usize, wifi: &WifiInfo, tint: Memo<Color>) -> AnyView {
    let live = state.live;
    let level = move |runtime: &mut Runtime| {
        live.with(runtime, |snapshot| {
            snapshot
                .tick
                .network
                .wifi
                .get(index)
                .and_then(|wifi| wifi.strength)
                .map_or(0.0, |strength| strength.get() / PERCENT)
        })
    };
    let live_signal = move |runtime: &mut Runtime| {
        live.with(runtime, |snapshot| {
            snapshot
                .tick
                .network
                .wifi
                .get(index)
                .map_or_else(String::new, signal)
        })
    };
    let name = wifi.ssid.as_deref().unwrap_or("Hidden network").to_owned();
    vstack((
        hstack((
            signal_meter(level, tint),
            text(format!("{name} · {}", wifi.interface)).font_semibold(),
        ))
        .gap(spacing::ROOMY)
        .items_center(),
        fact_row("Signal", bind(live_signal)),
        fact_row("Link rate", link_rate(wifi)),
        fact_row("Radio", radio(wifi)),
        fact_row("Security", security_name(wifi.security)),
        fact_row("BSSID", wifi.bssid.as_deref().unwrap_or("–").to_owned()),
    ))
    .gap(spacing::SNUG)
    .w_full()
    .role(Role::Group)
    .label(name)
    .boxed()
}

/// The connected access point per wireless interface, refreshed every 5 s by the engine.
pub fn wifi_panel(state: AppState, tint: Memo<Color>) -> impl View {
    let live = state.live;
    keyed(
        move |runtime| {
            live.with(runtime, |snapshot| {
                Arc::as_ptr(&snapshot.tick.network.wifi).cast::<()>() as usize
            })
        },
        move |cx, _| {
            let cards: Vec<AnyView> = live.with_untracked(cx, |snapshot| {
                snapshot
                    .tick
                    .network
                    .wifi
                    .iter()
                    .enumerate()
                    .map(|(index, wifi)| wifi_card(state, index, wifi, tint))
                    .collect()
            });
            if cards.is_empty() {
                return empty_state(
                    icons::WIFI_OFF,
                    "No Wi-Fi",
                    "No wireless interface is connected.",
                )
                .boxed();
            }
            vstack(cards).gap(spacing::SPACIOUS).w_full().boxed()
        },
    )
}
