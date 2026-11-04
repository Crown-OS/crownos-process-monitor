mod devices;
mod drive_actions;
mod health;
pub(in crate::pages) mod mirrored_history;
#[cfg(test)]
mod tests;
mod volumes;

use crownui::kit::{provide_toasts, toast_host};
use crownui::prelude::{Cx, Runtime, Signal, Styled, View, bind, hstack, vstack};
use monitor_engine::helper::Capability;
use monitor_engine::history::HistoryMetric;

use self::drive_actions::DriveActions;
use self::mirrored_history::mirrored_live_or_history;
use super::apps::apps_table;
use super::columns::{DISK_COLUMNS, DISK_SORT_INDEX};
use super::frame::{page_frame, panel};
use super::sources::{rate_readout, window_start_label, windowed_pair};
use crate::app::AppState;
use crate::app::history_query::history_totals;
use crate::app::live::{LiveSnapshot, latest};
use crate::components::app_table::RowId;
use crate::components::chart::{AxisLabels, Series, mirrored_chart};
use crate::components::coverage::{CoverageMark, LiveCoverage, live_coverage_badge};
use crate::components::fact_row::fact_row;
use crate::components::period_totals::{PeriodTotals, period_totals};
use crate::components::stat_strip::{Figure, stat_strip};
use crate::inspector;
use crate::util::format::{percent, rate};
use crate::util::spacing;
use crate::util::theme::Subsystem;

const BUSY_PERCENT: f32 = 90.0;

fn rate_figure(state: AppState, label: &'static str, pick: fn(&LiveSnapshot) -> f32) -> Figure {
    let (live, settings) = (state.live, state.settings);
    Figure::new(
        label,
        bind(move |runtime| {
            let unit = settings.get(runtime).rates;
            live.with(runtime, |snapshot| rate(f64::from(pick(snapshot)), unit))
        }),
    )
}

fn summary(state: AppState) -> impl View {
    let live = state.live;
    let busiest = move |runtime: &mut Runtime| {
        live.with(runtime, |snapshot| {
            snapshot.tick.disk.totals.utilization.get()
        })
    };
    let busy_text = busiest;
    stat_strip(
        rate_figure(state, "Read", |snapshot| latest(&snapshot.disk.read)),
        vec![
            rate_figure(state, "Write", |snapshot| latest(&snapshot.disk.write)),
            Figure::new(
                "IOPS",
                bind(move |runtime| {
                    live.with(runtime, |snapshot| {
                        let totals = &snapshot.tick.disk.totals;
                        format!("{:.0}", totals.read_iops + totals.write_iops)
                    })
                }),
            ),
            Figure::new(
                "Busiest disk",
                bind(move |runtime| percent(busy_text(runtime))),
            )
            .strain(bind(move |runtime| busiest(runtime) >= BUSY_PERCENT)),
        ],
    )
}

const THROUGHPUT_METRICS: [HistoryMetric; 2] = [HistoryMetric::DiskRead, HistoryMetric::DiskWrite];

/// Read above the axis, write below; History plots the stored rates instead.
fn throughput(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let series = [
        Series {
            label: "Read",
            color: state.tint(cx, Subsystem::Disk),
        },
        Series {
            label: "Write",
            color: state.series(cx, 4),
        },
    ];
    mirrored_live_or_history(cx, state, &THROUGHPUT_METRICS, series, move |_| {
        mirrored_chart(
            windowed_pair(
                state,
                |snapshot| &snapshot.disk.read,
                |snapshot| &snapshot.disk.write,
            ),
            series,
        )
        .readout(rate_readout(state))
        .labels(AxisLabels::new(
            "Read",
            bind(window_start_label(state)),
            "Write",
        ))
    })
}

/// Bytes read and written today, in 7 and in 30 days, from history while the page shows.
fn totals(cx: &mut Cx) -> impl View + use<> {
    let (data, driver) = history_totals(cx);
    let figures = period_totals(PeriodTotals {
        directions: ["read", "written"],
        totals: move |runtime: &mut Runtime| {
            data.with(runtime, |data| {
                data.ready().map(|totals| {
                    [totals.today, totals.last_7_days, totals.last_30_days].map(|period| {
                        let combined = period.combined();
                        [combined.disk_read.get(), combined.disk_written.get()]
                    })
                })
            })
        },
    });
    vstack((figures, driver)).w_full()
}

fn pressure(state: AppState) -> impl View {
    let live = state.live;
    let line = move |pick: fn(&monitor_engine::io::IoPressure) -> (f32, f32)| {
        bind(move |runtime| {
            live.with(runtime, |snapshot| {
                snapshot.tick.disk.pressure.as_ref().map_or_else(
                    || "Not available (PSI is off)".to_owned(),
                    |pressure| {
                        let (ten, sixty) = pick(pressure);
                        format!("{} over 10 s · {} over 60 s", percent(ten), percent(sixty))
                    },
                )
            })
        })
    };
    vstack((
        fact_row(
            "Some tasks waiting",
            line(|pressure| (pressure.some_avg10.get(), pressure.some_avg60.get())),
        ),
        fact_row(
            "All tasks stalled",
            line(|pressure| (pressure.full_avg10.get(), pressure.full_avg60.get())),
        ),
    ))
    .gap(spacing::SNUG)
    .w_full()
}

fn apps(cx: &mut Cx, state: AppState, inspected: Signal<Option<RowId>>) -> impl View + use<> {
    let badge = live_coverage_badge(
        state.live,
        LiveCoverage {
            coverage: |tick| tick.processes.io_coverage,
            partial: CoverageMark::OwnProcesses,
            capability: Capability::ReadAllProcesses,
        },
    );
    let table = apps_table(
        cx,
        state,
        "Disk apps",
        DISK_COLUMNS,
        DISK_SORT_INDEX,
        inspected,
    );
    vstack((hstack(badge), table))
        .gap(spacing::COMFORTABLE)
        .w_full()
}

pub fn page(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let inspected = cx.signal(None);
    let show_system = cx.signal(false);
    let tint = state.tint(cx, Subsystem::Disk);
    let strip = summary(state);
    let chart = throughput(cx, state);
    let totals = totals(cx);
    let table = apps(cx, state, inspected);
    let toasts = provide_toasts(cx);
    let drives = DriveActions::new(cx, toasts);
    let content = page_frame(
        cx,
        state,
        "Disk",
        (
            strip,
            panel("Throughput", chart),
            panel("Read and written", totals),
            panel(
                "Volumes",
                volumes::volumes_panel(state, tint, show_system, drives),
            ),
            panel("Disks", devices::devices_panel(state, drives)),
            panel("I/O pressure", pressure(state)),
            panel("Apps", table),
        ),
    );
    let content = toast_host(vstack((content, drives.view())).w_full().h_full())
        .w_full()
        .h_full();
    inspector::wrap(cx, state, inspected, content)
}
