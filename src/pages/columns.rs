//! App table columns over the live app list.

use std::fmt::Write;

use crate::app::live::{AppList, LiveSnapshot, ProcessDetail};
use crate::components::app_table::{Column, RowRef};
use crate::util::format::{write_bytes, write_count, write_percent};

fn cpu_of(apps: &AppList, row: RowRef) -> f64 {
    let percent = match row {
        RowRef::App(app) => apps.apps.get(app).map_or(0.0, |app| app.cpu_percent()),
        RowRef::Process(app, process) => apps
            .process(app, process)
            .map_or(0.0, |process| process.cpu_percent),
    };
    f64::from(percent)
}

fn memory_of(apps: &AppList, row: RowRef) -> f64 {
    let bytes = match row {
        RowRef::App(app) => apps.apps.get(app).map_or(0, |app| app.memory_bytes()),
        RowRef::Process(app, process) => apps
            .process(app, process)
            .map_or(0, |process| process.memory_bytes),
    };
    bytes as f64
}

fn threads_of(apps: &AppList, row: RowRef) -> f64 {
    let threads = match row {
        RowRef::App(app) => apps.apps.get(app).map_or(0, |app| app.threads()),
        RowRef::Process(app, process) => apps
            .process(app, process)
            .map_or(0, |process| process.threads),
    };
    f64::from(threads)
}

fn pid_of(apps: &AppList, row: RowRef) -> f64 {
    match row {
        RowRef::App(_) => 0.0,
        RowRef::Process(app, process) => apps
            .process(app, process)
            .map_or(0.0, |process| f64::from(process.pid)),
    }
}

fn write_cpu(apps: &AppList, row: RowRef, out: &mut String) {
    write_percent(out, cpu_of(apps, row) as f32);
}

fn write_memory(apps: &AppList, row: RowRef, out: &mut String) {
    write_bytes(out, memory_of(apps, row) as u64);
}

fn write_threads(apps: &AppList, row: RowRef, out: &mut String) {
    write_count(out, threads_of(apps, row) as u64);
}

fn write_pid(apps: &AppList, row: RowRef, out: &mut String) {
    if let RowRef::Process(app, process) = row
        && let Some(process) = apps.process(app, process)
    {
        write_count(out, u64::from(process.pid));
    }
}

fn write_user(apps: &AppList, row: RowRef, out: &mut String) {
    if let RowRef::Process(app, process) = row
        && let Some(process) = apps.process(app, process)
    {
        out.push_str(&process.user);
    }
}

const CPU: Column<AppList> = Column::metric("cpu", "% CPU", 72.0, write_cpu, cpu_of);
const MEMORY: Column<AppList> = Column::metric("memory", "Memory", 88.0, write_memory, memory_of);
const THREADS: Column<AppList> =
    Column::metric("threads", "Threads", 64.0, write_threads, threads_of).optional();
const PID: Column<AppList> = Column::detail("pid", "PID", 64.0, write_pid).sorted_by(pid_of);
const USER: Column<AppList> = Column::detail("user", "User", 72.0, write_user);

/// A process detail of `row`, summed over the app's processes for an app row.
pub fn detail_sum(snapshot: &LiveSnapshot, row: RowRef, pick: fn(&ProcessDetail) -> f64) -> f64 {
    let details = &snapshot.details;
    match row {
        RowRef::App(app) => details
            .processes
            .get(app)
            .map_or(0.0, |processes| processes.iter().map(pick).sum()),
        RowRef::Process(app, process) => pick(&details.process(app, process)),
    }
}

fn process_fact(apps: &AppList, row: RowRef) -> Option<&crate::app::live::ProcessFacts> {
    match row {
        RowRef::App(_) => None,
        RowRef::Process(app, process) => apps.process(app, process).map(|process| &process.facts),
    }
}

/// A per-process rate, summed over the app's processes for an app row.
fn rate_sum(apps: &AppList, row: RowRef, pick: fn(&crate::app::live::ProcessFacts) -> f32) -> f64 {
    match row {
        RowRef::App(app) => apps.apps.get(app).map_or(0.0, |app| {
            app.processes
                .iter()
                .map(|process| f64::from(pick(&process.facts)))
                .sum()
        }),
        RowRef::Process(..) => process_fact(apps, row).map_or(0.0, |facts| f64::from(pick(facts))),
    }
}

/// A per-process byte total, summed over the app's processes for an app row.
fn total_sum(apps: &AppList, row: RowRef, pick: fn(&crate::app::live::ProcessFacts) -> u64) -> f64 {
    let total = match row {
        RowRef::App(app) => apps.apps.get(app).map_or(0, |app| {
            app.processes
                .iter()
                .map(|process| pick(&process.facts))
                .sum()
        }),
        RowRef::Process(..) => process_fact(apps, row).map_or(0, pick),
    };
    total as f64
}

fn read_total_of(apps: &AppList, row: RowRef) -> f64 {
    total_sum(apps, row, |facts| facts.read_total)
}

fn written_total_of(apps: &AppList, row: RowRef) -> f64 {
    total_sum(apps, row, |facts| facts.written_total)
}

fn received_total_of(apps: &AppList, row: RowRef) -> f64 {
    total_sum(apps, row, |facts| facts.received_total)
}

fn sent_total_of(apps: &AppList, row: RowRef) -> f64 {
    total_sum(apps, row, |facts| facts.sent_total)
}

fn write_read_total(apps: &AppList, row: RowRef, out: &mut String) {
    write_bytes(out, read_total_of(apps, row) as u64);
}

fn write_written_total(apps: &AppList, row: RowRef, out: &mut String) {
    write_bytes(out, written_total_of(apps, row) as u64);
}

fn write_received_total(apps: &AppList, row: RowRef, out: &mut String) {
    write_bytes(out, received_total_of(apps, row) as u64);
}

fn write_sent_total(apps: &AppList, row: RowRef, out: &mut String) {
    write_bytes(out, sent_total_of(apps, row) as u64);
}

fn disk_read_of(apps: &AppList, row: RowRef) -> f64 {
    rate_sum(apps, row, |facts| facts.disk_read)
}

fn disk_write_of(apps: &AppList, row: RowRef) -> f64 {
    rate_sum(apps, row, |facts| facts.disk_write)
}

fn download_of(apps: &AppList, row: RowRef) -> f64 {
    rate_sum(apps, row, |facts| facts.download)
}

fn upload_of(apps: &AppList, row: RowRef) -> f64 {
    rate_sum(apps, row, |facts| facts.upload)
}

fn started_of(apps: &AppList, row: RowRef) -> f64 {
    let started = match row {
        RowRef::App(app) => apps.apps.get(app).map_or(0, |app| app.facts.started_unix),
        RowRef::Process(..) => process_fact(apps, row).map_or(0, |facts| facts.started_unix),
    };
    started as f64
}

fn nice_value_of(apps: &AppList, row: RowRef) -> f64 {
    process_fact(apps, row).map_or(0.0, |facts| f64::from(facts.nice))
}

fn write_rate_bytes(out: &mut String, bytes_per_second: f64) {
    crate::util::format::write_rate(out, bytes_per_second, crate::util::format::RateUnit::Bytes);
}

fn write_disk_read(apps: &AppList, row: RowRef, out: &mut String) {
    write_rate_bytes(out, disk_read_of(apps, row));
}

fn write_disk_write(apps: &AppList, row: RowRef, out: &mut String) {
    write_rate_bytes(out, disk_write_of(apps, row));
}

fn write_download(apps: &AppList, row: RowRef, out: &mut String) {
    write_rate_bytes(out, download_of(apps, row));
}

fn write_upload(apps: &AppList, row: RowRef, out: &mut String) {
    write_rate_bytes(out, upload_of(apps, row));
}

fn write_path(apps: &AppList, row: RowRef, out: &mut String) {
    if let Some(path) = process_fact(apps, row).and_then(|facts| facts.path.as_deref()) {
        out.push_str(path);
    }
}

fn write_started(apps: &AppList, row: RowRef, out: &mut String) {
    let started = started_of(apps, row) as i64;
    if started > 0 {
        crate::util::time::write_clock(out, started, monitor_engine::utc_offset_seconds());
    }
}

fn write_state(apps: &AppList, row: RowRef, out: &mut String) {
    if let Some(facts) = process_fact(apps, row) {
        out.push(facts.state.code());
    }
}

fn write_nice_value(apps: &AppList, row: RowRef, out: &mut String) {
    if let Some(facts) = process_fact(apps, row) {
        let _ = write!(out, "{}", facts.nice);
    }
}

pub const DISK_READ: Column<AppList> =
    Column::metric("disk-read", "Read/s", 84.0, write_disk_read, disk_read_of);
pub const DISK_WRITE: Column<AppList> = Column::metric(
    "disk-write",
    "Write/s",
    84.0,
    write_disk_write,
    disk_write_of,
);
pub const DOWNLOAD: Column<AppList> =
    Column::metric("download", "Down/s", 84.0, write_download, download_of);
pub const UPLOAD: Column<AppList> = Column::metric("upload", "Up/s", 84.0, write_upload, upload_of);
/// Totals since each process started, in the Columns menu.
pub const READ_TOTAL: Column<AppList> =
    Column::metric("read-total", "Read", 84.0, write_read_total, read_total_of).optional();
pub const WRITTEN_TOTAL: Column<AppList> = Column::metric(
    "written-total",
    "Written",
    84.0,
    write_written_total,
    written_total_of,
)
.optional();
pub const RECEIVED_TOTAL: Column<AppList> = Column::metric(
    "received-total",
    "Received",
    84.0,
    write_received_total,
    received_total_of,
)
.optional();
pub const SENT_TOTAL: Column<AppList> =
    Column::metric("sent-total", "Sent", 84.0, write_sent_total, sent_total_of).optional();
/// The Columns menu's process details: path, start time, state and nice.
pub const PATH: Column<AppList> = Column::detail("path", "Path", 180.0, write_path);
pub const STARTED: Column<AppList> =
    Column::detail("started", "Started", 64.0, write_started).sorted_by(started_of);
pub const STATE: Column<AppList> = Column::detail("state", "State", 44.0, write_state);
pub const NICE: Column<AppList> =
    Column::detail("nice", "Nice", 44.0, write_nice_value).sorted_by(nice_value_of);

pub const DISK_COLUMNS: &[Column<AppList>] = &[
    DISK_READ,
    DISK_WRITE,
    READ_TOTAL,
    WRITTEN_TOTAL,
    MEMORY,
    THREADS,
    PID,
    USER,
    PATH,
    STARTED,
    STATE,
    NICE,
];
pub const DISK_SORT_INDEX: usize = 0;
pub const NETWORK_COLUMNS: &[Column<AppList>] = &[
    DOWNLOAD,
    UPLOAD,
    RECEIVED_TOTAL,
    SENT_TOTAL,
    CPU,
    THREADS,
    PID,
    USER,
    PATH,
    STARTED,
    STATE,
    NICE,
];
pub const NETWORK_SORT_INDEX: usize = 0;

/// App-list columns that tables over the whole snapshot reuse through [`lift`].
const LIFTED: &[Column<AppList>] = &[CPU, MEMORY, THREADS, PID, USER, PATH, STARTED, STATE, NICE];

fn lifted_text<const INDEX: usize>(snapshot: &LiveSnapshot, row: RowRef, out: &mut String) {
    (LIFTED[INDEX].text)(&snapshot.apps, row, out);
}

fn lifted_sort<const INDEX: usize>(snapshot: &LiveSnapshot, row: RowRef) -> f64 {
    LIFTED[INDEX]
        .sort_key
        .map_or(0.0, |sort_key| sort_key(&snapshot.apps, row))
}

/// The `INDEX`th shared column, reading the snapshot's app list.
const fn lift<const INDEX: usize>() -> Column<LiveSnapshot> {
    let column = LIFTED[INDEX];
    Column {
        id: column.id,
        title: column.title,
        width: column.width,
        alignment: column.alignment,
        visibility: column.visibility,
        text: lifted_text::<INDEX>,
        sort_key: if column.sort_key.is_some() {
            Some(lifted_sort::<INDEX>)
        } else {
            None
        },
    }
}

pub const SNAPSHOT_CPU: Column<LiveSnapshot> = lift::<0>();
pub const SNAPSHOT_MEMORY: Column<LiveSnapshot> = lift::<1>();
pub const SNAPSHOT_THREADS: Column<LiveSnapshot> = lift::<2>();
pub const SNAPSHOT_PID: Column<LiveSnapshot> = lift::<3>();
pub const SNAPSHOT_USER: Column<LiveSnapshot> = lift::<4>();
/// Path, start time, state and nice, for the Columns menu.
pub const SNAPSHOT_DETAILS: [Column<LiveSnapshot>; 4] =
    [lift::<5>(), lift::<6>(), lift::<7>(), lift::<8>()];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_table::test_support::fixture_apps;

    #[test]
    fn since_start_totals_sum_over_an_app() {
        let mut apps = fixture_apps();
        for (index, process) in apps.apps[0].processes.iter_mut().enumerate() {
            process.facts.read_total = 1 << 20;
            process.facts.received_total = (index as u64 + 1) << 10;
        }
        assert_eq!(read_total_of(&apps, RowRef::App(0)), f64::from(3_u32 << 20));
        assert_eq!(received_total_of(&apps, RowRef::Process(0, 1)), 2048.0);
        let mut text = String::new();
        write_read_total(&apps, RowRef::App(0), &mut text);
        assert_eq!(text, "3.00 MB");
    }
}
