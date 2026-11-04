use crownui::prelude::{Cx, View, bind};
use monitor_engine::history::HistoryMetric;

use super::super::history_switch::{HistoryChart, live_or_history};
use super::super::sources::{percent_readout, window_start_label, windowed, windowed_many};
use crate::app::AppState;
use crate::components::chart::{AxisLabels, Series, YRange, area_chart, line_chart};
use crate::util::theme::Subsystem;

const FREQUENCY_HEIGHT: f32 = 110.0;

/// Total utilisation with the strain threshold drawn across it, or its stored history.
pub fn utilization(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let history = HistoryChart {
        subsystem: Subsystem::Cpu,
        range: YRange::PERCENT,
        readout: percent_readout,
    };
    live_or_history(cx, state, &[HistoryMetric::Cpu], history, move |cx| {
        utilization_chart(cx, state)
    })
}

fn utilization_chart(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let tint = state.tint(cx, Subsystem::Cpu);
    let limit = state
        .live
        .with_untracked(cx, |snapshot| snapshot.cpu.busy_limit);
    area_chart(windowed(state, |snapshot| &snapshot.cpu.total), tint)
        .range(YRange::PERCENT)
        .reference(limit)
        .readout(percent_readout)
        .labels(AxisLabels::new(
            "100%",
            bind(window_start_label(state)),
            "now",
        ))
}

/// Average and fastest-core clock over the window.
pub fn frequency_chart(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let series = vec![
        Series {
            label: "Average",
            color: state.tint(cx, Subsystem::Cpu),
        },
        Series {
            label: "Fastest core",
            color: state.tint(cx, Subsystem::Power),
        },
    ];
    line_chart(
        windowed_many(
            state,
            [
                |snapshot| &snapshot.cpu.frequency_average,
                |snapshot| &snapshot.cpu.frequency_peak,
            ],
        ),
        series,
    )
    .height(FREQUENCY_HEIGHT)
}
