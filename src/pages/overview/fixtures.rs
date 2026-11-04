//! Seeded engine data for the Overview's tests.

use std::sync::Arc;

use monitor_core::{AppKey, AppKind, Bytes, BytesPerSecond, Coverage, Percent, Watts};
use monitor_engine::Snapshot;
use monitor_engine::hardware::SystemDraw;
use monitor_engine::system::{AppRow, ExternalUsage, MemoryMethod, RoleBreakdown};

use crate::app::live::{HISTORY_CAPACITY, LiveSnapshot};

pub fn app_row(key: AppKey, name: &str, cpu_machine: f32, memory: u64) -> AppRow {
    AppRow {
        kind: key.kind(),
        key,
        name: name.into(),
        icon: None,
        process_count: 1,
        threads: 4,
        cpu: Percent::new(cpu_machine * 16.0),
        cpu_machine: Percent::new(cpu_machine),
        memory: Bytes::new(memory),
        memory_method: MemoryMethod::default(),
        resident: Bytes::new(memory),
        disk_read: BytesPerSecond::new(0.0),
        disk_write: BytesPerSecond::new(0.0),
        external: ExternalUsage::default(),
        roles: RoleBreakdown::default(),
        scope: None,
        frozen: false,
        started_unix: 0,
        coverage: Coverage::Full,
    }
}

fn firefox() -> AppKey {
    AppKey::new(AppKind::Desktop, "firefox")
}

/// A tick with three apps and a measured draw.
pub fn seeded_tick() -> Snapshot {
    let mut snapshot = Snapshot::default();
    snapshot.processes.apps = vec![
        app_row(firefox(), "Firefox", 20.0, 3 << 30),
        app_row(AppKey::new(AppKind::Desktop, "code"), "Code", 8.0, 1 << 30),
        app_row(
            AppKey::new(AppKind::Service, "pipewire"),
            "PipeWire",
            1.0,
            64 << 20,
        ),
    ];
    snapshot.system_draw = Some(SystemDraw {
        watts: Watts::new(14.0),
        coverage: Coverage::Full,
    });
    snapshot
}

/// Full-length live histories over the seeded tick.
pub fn seeded_live() -> LiveSnapshot {
    let mut live = LiveSnapshot::default();
    for history in [
        &mut live.cpu.total,
        &mut live.cpu.user,
        &mut live.cpu.system,
        &mut live.memory.used_percent,
        &mut live.gpu.busy,
        &mut live.disk.read,
        &mut live.disk.write,
        &mut live.network.download,
        &mut live.network.upload,
    ] {
        history.clear();
        history.resize(HISTORY_CAPACITY, 25.0);
    }
    live.memory.total_bytes = 15 << 30;
    live.memory.used_bytes = 6 << 30;
    let tick = Arc::new(seeded_tick());
    live.overview.append(&tick);
    live.tick = tick;
    live
}
