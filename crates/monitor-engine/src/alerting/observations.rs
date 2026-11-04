use monitor_alerts::{AlertInput, AppObservation};

use crate::Snapshot;

/// Refills `out` with this tick's per-app figures.
pub(super) fn fill(out: &mut Vec<AppObservation>, snapshot: &Snapshot) {
    out.clear();
    out.extend(snapshot.processes.apps.iter().map(|app| AppObservation {
        key: app.key.clone(),
        name: app.name.clone(),
        cpu_percent: app.cpu_machine,
        memory: app.memory,
        disk_bytes_per_second: app.disk_read + app.disk_write,
        network_bytes_per_second: app.external.network_receive + app.external.network_transmit,
        gpu_percent: app.external.gpu,
    }));
}

pub(super) fn input<'a>(snapshot: &Snapshot, apps: &'a [AppObservation]) -> AlertInput<'a> {
    let disk = &snapshot.disk.totals;
    let network = &snapshot.network.totals;
    AlertInput {
        wall_unix_seconds: snapshot
            .tick
            .map_or(0, |tick| tick.wall_unix_seconds() as i64),
        elapsed_seconds: if snapshot.elapsed.suspended {
            0.0
        } else {
            snapshot.elapsed.seconds
        },
        apps,
        cpu_percent: snapshot.cpu.total,
        gpu_percent: snapshot.busiest_gpu(),
        hottest_celsius: snapshot
            .sensors
            .hottest_sensor()
            .and_then(|reading| reading.value),
        pressure_level: snapshot.memory.pressure_level,
        disk_bytes_per_second: disk.read + disk.write,
        network_bytes_per_second: network.receive + network.transmit,
    }
}
