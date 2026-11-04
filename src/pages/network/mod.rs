mod interfaces;
mod routing;
mod socket_list;
mod sockets;
#[cfg(test)]
mod tests;
mod traffic;
mod wifi;

use crownui::prelude::{Cx, Runtime, Signal, Styled, View, bind, hstack, vstack};
use monitor_engine::helper::Capability;

use super::apps::apps_table;
use super::columns::{NETWORK_COLUMNS, NETWORK_SORT_INDEX};
use super::frame::{page_frame, panel};
use crate::app::AppState;
use crate::app::history_query::history_totals;
use crate::app::live::{LiveSnapshot, latest};
use crate::components::app_table::RowId;
use crate::components::coverage::{CoverageMark, LiveCoverage, live_coverage_badge};
use crate::components::period_totals::{PeriodTotals, period_totals};
use crate::components::stat_strip::{Figure, stat_strip};
use crate::inspector::{self, Inspector};
use crate::util::format::rate;
use crate::util::spacing;
use crate::util::theme::Subsystem;

fn rate_figure(state: AppState, label: &'static str, pick: fn(&LiveSnapshot) -> f64) -> Figure {
    let (live, settings) = (state.live, state.settings);
    Figure::new(
        label,
        bind(move |runtime| {
            let unit = settings.get(runtime).rates;
            live.with(runtime, |snapshot| rate(pick(snapshot), unit))
        }),
    )
}

fn summary(state: AppState) -> impl View {
    stat_strip(
        rate_figure(state, "Download", |snapshot| {
            f64::from(latest(&snapshot.network.download))
        }),
        vec![
            rate_figure(state, "Upload", |snapshot| {
                f64::from(latest(&snapshot.network.upload))
            }),
            rate_figure(state, "VPN down", |snapshot| {
                snapshot.tick.network.vpn_totals.receive.get()
            }),
            rate_figure(state, "VPN up", |snapshot| {
                snapshot.tick.network.vpn_totals.transmit.get()
            }),
        ],
    )
}

/// Bytes received and sent today, in 7 and in 30 days over the counted interfaces.
fn totals(cx: &mut Cx) -> impl View + use<> {
    let (data, driver) = history_totals(cx);
    let figures = period_totals(PeriodTotals {
        directions: ["received", "sent"],
        totals: move |runtime: &mut Runtime| {
            data.with(runtime, |data| {
                data.ready().map(|totals| {
                    [totals.today, totals.last_7_days, totals.last_30_days].map(|period| {
                        let combined = period.combined();
                        [
                            combined.network_received.get(),
                            combined.network_transmitted.get(),
                        ]
                    })
                })
            })
        },
    });
    vstack((figures, driver)).w_full()
}

fn apps(cx: &mut Cx, state: AppState, inspected: Signal<Option<RowId>>) -> impl View + use<> {
    let badge = live_coverage_badge(
        state.live,
        LiveCoverage {
            coverage: |tick| tick.network.process_traffic_coverage,
            partial: CoverageMark::TcpOnly,
            capability: Capability::ReadNetworkAccounting,
        },
    );
    let table = apps_table(
        cx,
        state,
        "Network apps",
        NETWORK_COLUMNS,
        NETWORK_SORT_INDEX,
        inspected,
    );
    vstack((hstack(badge), table))
        .gap(spacing::COMFORTABLE)
        .w_full()
}

/// Traffic, interfaces, routes, Wi-Fi, sockets and apps. A public IP is never looked up: it
/// would need a request to an outside server.
pub fn page(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let inspected = cx.signal(None);
    let inspector = Inspector::new(cx);
    inspector::follow_rows(cx, state, inspector, inspected);
    let tint = state.tint(cx, Subsystem::Network);
    let strip = summary(state);
    let traffic = traffic::traffic_panel(cx, state);
    let totals = totals(cx);
    let listening = sockets::listening_panel(cx, state, inspector.target);
    let connections = sockets::connections_panel(cx, state, inspector.target);
    let table = apps(cx, state, inspected);
    let interfaces = interfaces::interfaces_panel(cx, state);
    let content = page_frame(
        cx,
        state,
        "Network",
        (
            strip,
            panel("Traffic", traffic),
            panel("Received and sent", totals),
            panel("Interfaces", interfaces),
            panel("Routes and DNS", routing::routing_panel(state)),
            panel("Wi-Fi", wifi::wifi_panel(state, tint)),
            panel("Listening ports", listening),
            panel("Connections", connections),
            panel("Apps", table),
        ),
    );
    inspector::inspector_drawer(cx, state, inspector, content)
}
