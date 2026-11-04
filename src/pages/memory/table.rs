use crownui::prelude::{Cx, Signal, Styled, View, hstack, text, vstack};
use monitor_engine::helper::Capability;
use monitor_engine::system::MemoryMethod;

use super::super::apps::apps_table;
use super::super::columns::{
    SNAPSHOT_CPU, SNAPSHOT_DETAILS, SNAPSHOT_PID, SNAPSHOT_USER, detail_sum,
};
use crate::app::AppState;
use crate::app::live::LiveSnapshot;
use crate::components::app_table::{Column, RowId, RowRef, Visibility};
use crate::components::coverage::{CoverageMark, LiveCoverage, live_coverage_badge};
use crate::components::top_app::top_app;
use crate::pages::sources::whole_snapshot;
use crate::util::format::write_bytes;
use crate::util::spacing;

const NOTE_SIZE: f32 = 12.0;
pub const METHOD_NOTE: &str =
    "cgroup is exact · PSS splits shared pages fairly · RSS ≈ counts shared pages in every process";

pub const fn method_label(method: MemoryMethod) -> &'static str {
    match method {
        MemoryMethod::Cgroup => "cgroup",
        MemoryMethod::Pss => "PSS",
        MemoryMethod::Rss => "RSS ≈",
    }
}

/// An app's own measurement (exact for a cgroup); a process's PSS, else its RSS.
fn memory_of(snapshot: &LiveSnapshot, row: RowRef) -> f64 {
    match row {
        RowRef::App(app) => snapshot.details.app(app).memory as f64,
        RowRef::Process(..) => detail_sum(snapshot, row, |detail| {
            detail.proportional.unwrap_or(detail.resident) as f64
        }),
    }
}

fn resident_of(snapshot: &LiveSnapshot, row: RowRef) -> f64 {
    detail_sum(snapshot, row, |detail| detail.resident as f64)
}

fn proportional_of(snapshot: &LiveSnapshot, row: RowRef) -> f64 {
    detail_sum(snapshot, row, |detail| {
        detail.proportional.unwrap_or(0) as f64
    })
}

fn unique_of(snapshot: &LiveSnapshot, row: RowRef) -> f64 {
    detail_sum(snapshot, row, |detail| detail.unique.unwrap_or(0) as f64)
}

fn swap_of(snapshot: &LiveSnapshot, row: RowRef) -> f64 {
    detail_sum(snapshot, row, |detail| detail.swap.unwrap_or(0) as f64)
}

fn peak_of(snapshot: &LiveSnapshot, row: RowRef) -> f64 {
    detail_sum(snapshot, row, |detail| {
        detail.peak_resident.unwrap_or(0) as f64
    })
}

fn write_amount(out: &mut String, value: f64) {
    if value > 0.0 {
        write_bytes(out, value as u64);
    }
}

fn write_memory(snapshot: &LiveSnapshot, row: RowRef, out: &mut String) {
    write_amount(out, memory_of(snapshot, row));
}

fn write_method(snapshot: &LiveSnapshot, row: RowRef, out: &mut String) {
    if let RowRef::App(app) = row
        && app < snapshot.details.apps.len()
    {
        out.push_str(method_label(snapshot.details.app(app).memory_method));
    }
}

fn write_resident(snapshot: &LiveSnapshot, row: RowRef, out: &mut String) {
    write_amount(out, resident_of(snapshot, row));
}

fn write_proportional(snapshot: &LiveSnapshot, row: RowRef, out: &mut String) {
    write_amount(out, proportional_of(snapshot, row));
}

fn write_unique(snapshot: &LiveSnapshot, row: RowRef, out: &mut String) {
    write_amount(out, unique_of(snapshot, row));
}

fn write_swap(snapshot: &LiveSnapshot, row: RowRef, out: &mut String) {
    write_amount(out, swap_of(snapshot, row));
}

fn write_peak(snapshot: &LiveSnapshot, row: RowRef, out: &mut String) {
    write_amount(out, peak_of(snapshot, row));
}

const MEMORY: Column<LiveSnapshot> =
    Column::metric("memory", "Memory", 88.0, write_memory, memory_of);
const METHOD: Column<LiveSnapshot> = Column {
    visibility: Visibility::Always,
    ..Column::detail("method", "Method", 64.0, write_method)
};
const RESIDENT: Column<LiveSnapshot> =
    Column::metric("rss", "RSS", 80.0, write_resident, resident_of);
const PROPORTIONAL: Column<LiveSnapshot> =
    Column::metric("pss", "PSS", 80.0, write_proportional, proportional_of);
const UNIQUE: Column<LiveSnapshot> =
    Column::metric("uss", "USS", 80.0, write_unique, unique_of).optional();
const SWAP: Column<LiveSnapshot> =
    Column::metric("swap", "Swap", 72.0, write_swap, swap_of).optional();
const PEAK: Column<LiveSnapshot> =
    Column::metric("peak", "Peak", 80.0, write_peak, peak_of).optional();

pub const MEMORY_TABLE: &[Column<LiveSnapshot>] = &[
    MEMORY,
    METHOD,
    RESIDENT,
    PROPORTIONAL,
    UNIQUE,
    SWAP,
    PEAK,
    SNAPSHOT_CPU,
    SNAPSHOT_PID,
    SNAPSHOT_USER,
    SNAPSHOT_DETAILS[0],
    SNAPSHOT_DETAILS[1],
    SNAPSHOT_DETAILS[2],
    SNAPSHOT_DETAILS[3],
];

/// The Top App by memory, the method legend and coverage, over the table.
pub fn apps_panel(
    cx: &mut Cx,
    state: AppState,
    inspected: Signal<Option<RowId>>,
) -> impl View + use<> {
    let own_only = live_coverage_badge(
        state.live,
        LiveCoverage {
            coverage: |tick| tick.processes.memory_coverage,
            partial: CoverageMark::OwnProcesses,
            capability: Capability::ReadAllProcesses,
        },
    );
    vstack((
        hstack((top_app(state.live, whole_snapshot, MEMORY), own_only))
            .gap(spacing::LOOSE)
            .items_center(),
        text(METHOD_NOTE).text_size(NOTE_SIZE),
        apps_table(cx, state, "Memory apps", MEMORY_TABLE, 0, inspected),
    ))
    .gap(spacing::ROOMY)
    .w_full()
}
