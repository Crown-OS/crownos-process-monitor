use std::sync::Arc;

use crownui::kit::{StatusOptions, Tone, icons, status};
use crownui::prelude::{
    AnyView, Cx, Role, Runtime, Semantic, Signal, Styled, View, bind, hstack, keyed, spacer, text,
    vstack,
};
use monitor_engine::io::{Duplex, InterfaceInfo, InterfaceKind, NetworkInterface, OperState};

use crate::app::AppState;
use crate::app::history_query::{HistoryData, InterfacePeriods, history_interface_totals};
use crate::components::empty_state::empty_state;
use crate::components::fact_row::fact_row;
use crate::components::period_totals::PERIOD_LABELS;
use crate::settings::MonitorSettings;
use crate::util::format::{bytes, count, rate};
use crate::util::spacing;

const NAME_SIZE: f32 = 14.0;

pub const fn kind_name(kind: InterfaceKind) -> &'static str {
    match kind {
        InterfaceKind::Wifi => "Wi-Fi",
        InterfaceKind::Ethernet => "Ethernet",
        InterfaceKind::Vpn => "VPN",
        InterfaceKind::Bridge => "Bridge",
        InterfaceKind::Virtual => "Virtual",
        InterfaceKind::Loopback => "Loopback",
        InterfaceKind::Mobile => "Mobile",
        InterfaceKind::Other => "Other",
    }
}

pub const fn state_badge(state: OperState) -> (&'static str, Tone) {
    match state {
        OperState::Up => ("Up", Tone::Success),
        OperState::Dormant => ("Dormant", Tone::Warning),
        OperState::Testing => ("Testing", Tone::Warning),
        OperState::Down | OperState::LowerLayerDown => ("Down", Tone::Neutral),
        OperState::NotPresent => ("Not present", Tone::Neutral),
        OperState::Unknown => ("Unknown", Tone::Neutral),
    }
}

fn link(info: &InterfaceInfo) -> String {
    let speed = info
        .speed_mbps
        .map_or_else(String::new, |speed| format!("{speed} Mb/s"));
    let duplex = match info.duplex {
        Duplex::Full => "full duplex",
        Duplex::Half => "half duplex",
        Duplex::Unknown => "",
    };
    let mtu = info
        .mtu
        .map_or_else(String::new, |mtu| format!("MTU {mtu}"));
    [speed.as_str(), duplex, mtu.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn addresses(info: &InterfaceInfo) -> String {
    info.addresses
        .iter()
        .map(|address| format!("{}/{}", address.address, address.prefix_length))
        .collect::<Vec<_>>()
        .join("\n")
}

fn interface_figure(
    state: AppState,
    index: usize,
    write: fn(&NetworkInterface, MonitorSettings) -> String,
) -> impl Fn(&mut Runtime) -> String + 'static {
    let (live, settings) = (state.live, state.settings);
    move |runtime| {
        let settings = settings.get(runtime);
        live.with(runtime, |snapshot| {
            snapshot
                .tick
                .network
                .interfaces
                .get(index)
                .map_or_else(String::new, |interface| write(interface, settings))
        })
    }
}

fn throughput(interface: &NetworkInterface, settings: MonitorSettings) -> String {
    format!(
        "{} down · {} up",
        rate(interface.rates.receive.get(), settings.rates),
        rate(interface.rates.transmit.get(), settings.rates)
    )
}

fn packets(interface: &NetworkInterface, _: MonitorSettings) -> String {
    let counters = &interface.counters;
    format!(
        "{} in · {} out · {:.0}/s",
        count(counters.receive_packets),
        count(counters.transmit_packets),
        interface.rates.receive_packets + interface.rates.transmit_packets
    )
}

fn problems(interface: &NetworkInterface, _: MonitorSettings) -> String {
    let counters = &interface.counters;
    format!(
        "{} errors · {} dropped",
        count(counters.receive_errors + counters.transmit_errors),
        count(counters.receive_drops + counters.transmit_drops)
    )
}

fn totals(interface: &NetworkInterface, _: MonitorSettings) -> String {
    format!(
        "{} received · {} sent",
        bytes(interface.counters.receive_bytes.get()),
        bytes(interface.counters.transmit_bytes.get())
    )
}

/// "1.2 GB received · 80 MB sent" for one interface and period, from history.
fn period_text(
    history: Signal<HistoryData<Vec<InterfacePeriods>>>,
    name: Arc<str>,
    period: usize,
) -> impl Fn(&mut Runtime) -> String + 'static {
    move |runtime| {
        history.with(runtime, |history| match history {
            HistoryData::Ready(interfaces) => interfaces
                .iter()
                .find(|interface| interface.interface == name)
                .map_or_else(
                    || "Not recorded".to_owned(),
                    |interface| {
                        let [received, sent] = interface.periods[period];
                        format!("{} received · {} sent", bytes(received), bytes(sent))
                    },
                ),
            HistoryData::Loading => "–".to_owned(),
            HistoryData::Unavailable => "History is not available".to_owned(),
        })
    }
}

fn interface_card(
    state: AppState,
    index: usize,
    interface: &NetworkInterface,
    history: Signal<HistoryData<Vec<InterfacePeriods>>>,
) -> AnyView {
    let info = &interface.info;
    let name = info.name.to_string();
    let (state_label, tone) = state_badge(interface.state);
    let kind = info
        .link_kind
        .as_deref()
        .filter(|kind| !kind.is_empty())
        .map_or_else(
            || kind_name(info.kind).to_owned(),
            |link_kind| format!("{} ({link_kind})", kind_name(info.kind)),
        );
    let excluded = (!info.kind.counts_in_totals()).then(|| {
        status(if info.kind.is_tunnel() {
            "Counted separately"
        } else {
            "Not in totals"
        })
        .tone(Tone::Neutral)
    });
    vstack((
        hstack((
            text(format!("{name} · {kind}"))
                .text_size(NAME_SIZE)
                .font_semibold()
                .truncate(),
            spacer(),
            excluded,
            status(state_label).tone(tone),
        ))
        .gap(spacing::COMFORTABLE)
        .items_center(),
        fact_row(
            "Throughput",
            bind(interface_figure(state, index, throughput)),
        ),
        fact_row("Packets", bind(interface_figure(state, index, packets))),
        fact_row("Errors", bind(interface_figure(state, index, problems))),
        fact_row("Since up", bind(interface_figure(state, index, totals))),
        vstack(
            PERIOD_LABELS
                .iter()
                .enumerate()
                .map(|(period, label)| {
                    let shown = period_text(history, Arc::clone(&info.name), period);
                    fact_row(label, bind(shown)).boxed()
                })
                .collect::<Vec<_>>(),
        )
        .gap(spacing::SNUG)
        .w_full(),
        fact_row("MAC", info.mac.as_deref().unwrap_or("–").to_owned()),
        fact_row("Link", link(info)),
        fact_row("Addresses", {
            let listed = addresses(info);
            if listed.is_empty() {
                "–".to_owned()
            } else {
                listed
            }
        }),
    ))
    .gap(spacing::SNUG)
    .w_full()
    .role(Role::Group)
    .label(name)
    .boxed()
}

/// Every interface but loopback, with its traffic today, in 7 and in 30 days from history;
/// rebuilt when interfaces appear, vanish or change address.
pub fn interfaces_panel(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    let (history, driver) = history_interface_totals(cx);
    let cards = keyed(
        move |runtime| {
            live.with(runtime, |snapshot| {
                snapshot
                    .tick
                    .network
                    .interfaces
                    .iter()
                    .map(|interface| Arc::as_ptr(&interface.info) as usize)
                    .fold(0_usize, |hash, pointer| hash.rotate_left(7) ^ pointer)
            })
        },
        move |cx, _| {
            let cards: Vec<AnyView> = live.with_untracked(cx, |snapshot| {
                snapshot
                    .tick
                    .network
                    .interfaces
                    .iter()
                    .enumerate()
                    .filter(|(_, interface)| interface.info.kind != InterfaceKind::Loopback)
                    .map(|(index, interface)| interface_card(state, index, interface, history))
                    .collect()
            });
            if cards.is_empty() {
                return empty_state(icons::NETWORK, "No interfaces", "No network links are up.")
                    .boxed();
            }
            vstack(cards).gap(spacing::GENEROUS).w_full().boxed()
        },
    );
    vstack((cards, driver)).w_full()
}
