use crownui::prelude::{Cx, Signal, Styled, View, vstack};

use super::super::apps::apps_table;
use super::super::columns::{
    SNAPSHOT_CPU, SNAPSHOT_DETAILS, SNAPSHOT_MEMORY, SNAPSHOT_PID, SNAPSHOT_THREADS, SNAPSHOT_USER,
    detail_sum,
};
use crate::app::AppState;
use crate::app::live::LiveSnapshot;
use crate::components::app_table::{Column, RowId, RowRef, Visibility};
use crate::components::top_app::top_app;
use crate::pages::sources::whole_snapshot;
use crate::util::format::write_duration;
use crate::util::spacing;

fn cpu_time_of(snapshot: &LiveSnapshot, row: RowRef) -> f64 {
    detail_sum(snapshot, row, |detail| detail.cpu_seconds as f64)
}

fn write_cpu_time(snapshot: &LiveSnapshot, row: RowRef, out: &mut String) {
    write_duration(out, cpu_time_of(snapshot, row) as u64);
}

const CPU_TIME: Column<LiveSnapshot> =
    Column::metric("cpu-time", "CPU Time", 80.0, write_cpu_time, cpu_time_of);

/// Nice is always shown on the CPU page; path, start time and state stay in the Columns menu.
const NICE: Column<LiveSnapshot> = Column {
    visibility: Visibility::Always,
    ..SNAPSHOT_DETAILS[3]
};

pub const CPU_TABLE: &[Column<LiveSnapshot>] = &[
    SNAPSHOT_CPU,
    CPU_TIME,
    SNAPSHOT_THREADS,
    NICE,
    SNAPSHOT_MEMORY,
    SNAPSHOT_PID,
    SNAPSHOT_USER,
    SNAPSHOT_DETAILS[0],
    SNAPSHOT_DETAILS[1],
    SNAPSHOT_DETAILS[2],
];

/// The Top App by CPU over the table of every app.
pub fn apps_panel(
    cx: &mut Cx,
    state: AppState,
    inspected: Signal<Option<RowId>>,
) -> impl View + use<> {
    vstack((
        top_app(state.live, whole_snapshot, SNAPSHOT_CPU),
        apps_table(cx, state, "CPU apps", CPU_TABLE, 0, inspected),
    ))
    .gap(spacing::ROOMY)
    .w_full()
}
