use std::cmp::Ordering;

use super::row::AppRow;
use crate::process::{ProcessRow, ProcessTable};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    Ascending,
    #[default]
    Descending,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AppSortKey {
    Name,
    ProcessCount,
    #[default]
    Cpu,
    Memory,
    DiskRead,
    DiskWrite,
    Disk,
    Gpu,
    GpuMemory,
    NetworkReceive,
    NetworkTransmit,
    Network,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ProcessSortKey {
    Name,
    Pid,
    #[default]
    Cpu,
    CpuTime,
    Memory,
    Threads,
    Nice,
    Started,
    Disk,
    Gpu,
    Network,
}

/// Case-insensitive, allocation-free name order.
fn compare_names(left: &str, right: &str) -> Ordering {
    left.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(right.bytes().map(|byte| byte.to_ascii_lowercase()))
}

fn compare_app(left: &AppRow, right: &AppRow, key: AppSortKey) -> Ordering {
    let float = |value: fn(&AppRow) -> f64| value(left).total_cmp(&value(right));
    match key {
        AppSortKey::Name => compare_names(&left.name, &right.name),
        AppSortKey::ProcessCount => left.process_count.cmp(&right.process_count),
        AppSortKey::Cpu => float(|row| f64::from(row.cpu.get())),
        AppSortKey::Memory => left.memory.cmp(&right.memory),
        AppSortKey::DiskRead => float(|row| row.disk_read.get()),
        AppSortKey::DiskWrite => float(|row| row.disk_write.get()),
        AppSortKey::Disk => float(|row| row.disk_read.get() + row.disk_write.get()),
        AppSortKey::Gpu => float(|row| f64::from(row.external.gpu.get())),
        AppSortKey::GpuMemory => left.external.gpu_memory.cmp(&right.external.gpu_memory),
        AppSortKey::NetworkReceive => float(|row| row.external.network_receive.get()),
        AppSortKey::NetworkTransmit => float(|row| row.external.network_transmit.get()),
        AppSortKey::Network => {
            float(|row| row.external.network_receive.get() + row.external.network_transmit.get())
        }
    }
}

fn compare_process(left: &ProcessRow, right: &ProcessRow, key: ProcessSortKey) -> Ordering {
    let float = |value: fn(&ProcessRow) -> f64| value(left).total_cmp(&value(right));
    match key {
        ProcessSortKey::Name => compare_names(&left.name, &right.name),
        ProcessSortKey::Pid => left.key.pid.cmp(&right.key.pid),
        ProcessSortKey::Cpu => float(|row| f64::from(row.cpu.get())),
        ProcessSortKey::CpuTime => left.cpu_time.cmp(&right.cpu_time),
        ProcessSortKey::Memory => left.memory().cmp(&right.memory()),
        ProcessSortKey::Threads => left.threads.cmp(&right.threads),
        ProcessSortKey::Nice => left.nice.cmp(&right.nice),
        ProcessSortKey::Started => left.started_unix.cmp(&right.started_unix),
        ProcessSortKey::Disk => float(|row| row.disk_read.get() + row.disk_write.get()),
        ProcessSortKey::Gpu => float(|row| f64::from(row.external.gpu.get())),
        ProcessSortKey::Network => {
            float(|row| row.external.network_receive.get() + row.external.network_transmit.get())
        }
    }
}

fn directed(ordering: Ordering, direction: Direction) -> Ordering {
    match direction {
        Direction::Ascending => ordering,
        Direction::Descending => ordering.reverse(),
    }
}

/// Fills `order` with app indices sorted by `key`, leaving `apps` in place.
/// Ties keep index order, so the result is stable across ticks.
pub fn sort_apps(apps: &[AppRow], key: AppSortKey, direction: Direction, order: &mut Vec<usize>) {
    order.clear();
    order.extend(0..apps.len());
    order.sort_unstable_by(|&left, &right| {
        directed(compare_app(&apps[left], &apps[right], key), direction).then(left.cmp(&right))
    });
}

/// Fills `order` with the indices (into `table.processes`) of the app's
/// processes, sorted by `key`.
pub fn sort_processes_of_app(
    table: &ProcessTable,
    app: usize,
    key: ProcessSortKey,
    direction: Direction,
    order: &mut Vec<usize>,
) {
    order.clear();
    order.extend(
        table
            .processes
            .iter()
            .enumerate()
            .filter(|(_, row)| row.app == app)
            .map(|(index, _)| index),
    );
    let rows = &table.processes;
    order.sort_unstable_by(|&left, &right| {
        directed(compare_process(&rows[left], &rows[right], key), direction).then(left.cmp(&right))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_names_without_case() {
        assert_eq!(compare_names("alpha", "Beta"), Ordering::Less);
        assert_eq!(compare_names("Kitty", "kitty"), Ordering::Equal);
    }
}
