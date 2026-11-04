use crownui::prelude::{Cx, Signal, Styled, View, bind, hstack, show, text, vstack};
use monitor_core::Coverage;
use monitor_engine::helper::Capability;

use super::super::apps::apps_table;
use super::super::columns::{
    SNAPSHOT_CPU, SNAPSHOT_DETAILS, SNAPSHOT_PID, SNAPSHOT_USER, detail_sum,
};
use crate::app::AppState;
use crate::app::live::{GpuUse, LiveSnapshot};
use crate::components::app_table::{Column, RowId, RowRef, Visibility};
use crate::components::coverage::{CoverageMark, LiveCoverage, live_coverage_badge};
use crate::components::top_app::top_app;
use crate::pages::sources::whole_snapshot;
use crate::util::format::{write_bytes, write_percent};
use crate::util::spacing;

const NOTE_SIZE: f32 = 12.0;
pub const COVERAGE_NOTE: &str = "GPU use of other users' processes needs the helper.";

fn gpu_of(snapshot: &LiveSnapshot, row: RowRef) -> f64 {
    let apps = &snapshot.apps;
    let percent = match row {
        RowRef::App(app) => apps.apps.get(app).map_or(0.0, |app| {
            app.processes
                .iter()
                .map(|process| process.facts.gpu_percent)
                .sum()
        }),
        RowRef::Process(app, process) => apps
            .process(app, process)
            .map_or(0.0, |process| process.facts.gpu_percent),
    };
    f64::from(percent)
}

fn gpu_memory_of(snapshot: &LiveSnapshot, row: RowRef) -> f64 {
    detail_sum(snapshot, row, |detail| detail.gpu_memory as f64)
}

/// Which kinds of GPU the row uses: an app combines its processes.
pub fn gpu_use_of(snapshot: &LiveSnapshot, row: RowRef) -> GpuUse {
    let details = &snapshot.details;
    match row {
        RowRef::App(app) => details
            .processes
            .get(app)
            .map_or(GpuUse::None, |processes| {
                processes
                    .iter()
                    .fold(GpuUse::None, |used, detail| used.combined(detail.gpu))
            }),
        RowRef::Process(app, process) => details.process(app, process).gpu,
    }
}

fn write_gpu(snapshot: &LiveSnapshot, row: RowRef, out: &mut String) {
    write_percent(out, gpu_of(snapshot, row) as f32);
}

fn write_gpu_memory(snapshot: &LiveSnapshot, row: RowRef, out: &mut String) {
    let bytes = gpu_memory_of(snapshot, row);
    if bytes > 0.0 {
        write_bytes(out, bytes as u64);
    }
}

fn write_gpu_use(snapshot: &LiveSnapshot, row: RowRef, out: &mut String) {
    out.push_str(gpu_use_of(snapshot, row).label());
}

const GPU: Column<LiveSnapshot> = Column::metric("gpu", "% GPU", 72.0, write_gpu, gpu_of);
const GPU_MEMORY: Column<LiveSnapshot> = Column::metric(
    "gpu-memory",
    "GPU Memory",
    96.0,
    write_gpu_memory,
    gpu_memory_of,
);
const GPU_USED: Column<LiveSnapshot> = Column {
    visibility: Visibility::Always,
    ..Column::detail("gpu-used", "GPU", 88.0, write_gpu_use)
};

pub const GPU_TABLE: &[Column<LiveSnapshot>] = &[
    GPU,
    GPU_MEMORY,
    GPU_USED,
    SNAPSHOT_CPU,
    SNAPSHOT_PID,
    SNAPSHOT_USER,
    SNAPSHOT_DETAILS[0],
    SNAPSHOT_DETAILS[1],
    SNAPSHOT_DETAILS[2],
    SNAPSHOT_DETAILS[3],
];

/// The Top App by GPU, the coverage of per-process figures, then the table.
pub fn apps_panel(
    cx: &mut Cx,
    state: AppState,
    inspected: Signal<Option<RowId>>,
) -> impl View + use<> {
    let live = state.live;
    let own_only = show(
        bind(move |runtime| {
            live.with(runtime, |snapshot| {
                snapshot.tick.gpu.process_coverage == Coverage::OwnProcesses
            })
        }),
        || text(COVERAGE_NOTE).text_size(NOTE_SIZE),
        || vstack(()),
    );
    let badge = live_coverage_badge(
        live,
        LiveCoverage {
            coverage: |tick| tick.gpu.process_coverage,
            partial: CoverageMark::OwnProcesses,
            capability: Capability::ReadGpuCounters,
        },
    );
    vstack((
        top_app(state.live, whole_snapshot, GPU),
        hstack((badge, own_only))
            .gap(spacing::COMFORTABLE)
            .items_center(),
        apps_table(cx, state, "GPU apps", GPU_TABLE, 0, inspected),
    ))
    .gap(spacing::ROOMY)
    .w_full()
}
