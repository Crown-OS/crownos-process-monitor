use std::sync::Arc;

use crownui::kit::{ButtonOptions, ButtonSize, ButtonVariant, button};
use crownui::prelude::{
    AnyView, Cx, Listeners, Semantic, Styled, View, bind, hstack, keyed, vstack,
};
use monitor_engine::history::HistoryMetric;

use super::interfaces::kind_name;
use crate::app::AppState;
use crate::components::chart::{AxisLabels, Series, mirrored_chart};
use crate::pages::disk::mirrored_history::mirrored_live_or_history;
use crate::pages::sources::{rate_readout, window_start_label, windowed_pair};
use crate::util::spacing;
use crate::util::theme::Subsystem;

/// One chip per charted interface; a click adds it to or drops it from the chart's sum.
fn toggles(state: AppState) -> impl View {
    let live = state.live;
    keyed(
        move |runtime| {
            live.with(runtime, |snapshot| {
                snapshot
                    .network
                    .interfaces
                    .iter()
                    .map(|history| {
                        (
                            Arc::as_ptr(&history.name).cast::<()>() as usize,
                            history.shown,
                        )
                    })
                    .fold(0_usize, |hash, (name, shown)| {
                        hash.rotate_left(5) ^ name ^ usize::from(shown)
                    })
            })
        },
        move |cx, _| {
            let chips: Vec<AnyView> = live.with_untracked(cx, |snapshot| {
                snapshot
                    .network
                    .interfaces
                    .iter()
                    .map(|history| {
                        let name = Arc::clone(&history.name);
                        let variant = if history.shown {
                            ButtonVariant::Tinted
                        } else {
                            ButtonVariant::Ghost
                        };
                        button(format!("{} · {}", history.name, kind_name(history.kind)))
                            .variant(variant)
                            .control_size(ButtonSize::Small)
                            .value(if history.shown { "shown" } else { "hidden" })
                            .on_click(move |cx: &mut Cx, _| {
                                live.update(cx, |snapshot| {
                                    snapshot.network.toggle_interface(&name)
                                });
                            })
                            .boxed()
                    })
                    .collect()
            });
            hstack(chips).gap(spacing::SNUG).flex_wrap()
        },
    )
}

const TRAFFIC_METRICS: [HistoryMetric; 2] = [
    HistoryMetric::NetworkReceive,
    HistoryMetric::NetworkTransmit,
];

/// Download above the axis, upload below, summed over the toggled interfaces; History plots
/// the stored totals instead.
pub fn traffic_panel(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let series = [
        Series {
            label: "Down",
            color: state.tint(cx, Subsystem::Network),
        },
        Series {
            label: "Up",
            color: state.series(cx, 1),
        },
    ];
    let chart = mirrored_live_or_history(cx, state, &TRAFFIC_METRICS, series, move |_| {
        mirrored_chart(
            windowed_pair(
                state,
                |snapshot| &snapshot.network.shown_download,
                |snapshot| &snapshot.network.shown_upload,
            ),
            series,
        )
        .readout(rate_readout(state))
        .labels(AxisLabels::new(
            "Download",
            bind(window_start_label(state)),
            "Upload",
        ))
    });
    vstack((chart, toggles(state))).gap(spacing::ROOMY).w_full()
}
