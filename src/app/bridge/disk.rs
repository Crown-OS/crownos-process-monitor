//! Disk throughput histories and the root volume; the Disk page reads devices, volumes, health
//! and pressure straight from the latest engine snapshot.

use monitor_engine::Snapshot;

use crate::app::live::{HISTORY_CAPACITY, push_sample};

const ROOT_MOUNT: &str = "/";

/// Rates are bytes per second.
#[derive(Debug, Default)]
pub struct DiskReadings {
    pub read: Vec<f32>,
    pub write: Vec<f32>,
    pub used_bytes: u64,
    pub total_bytes: u64,
}

impl DiskReadings {
    pub fn prepare(&mut self) {
        for history in [&mut self.read, &mut self.write] {
            history.clear();
            history.resize(HISTORY_CAPACITY, f32::NAN);
        }
    }

    pub fn push(&mut self, snapshot: &Snapshot) {
        let totals = &snapshot.disk.totals;
        push_sample(&mut self.read, totals.read.get() as f32);
        push_sample(&mut self.write, totals.write.get() as f32);
    }

    pub fn push_gap(&mut self) {
        push_sample(&mut self.read, f32::NAN);
        push_sample(&mut self.write, f32::NAN);
    }

    pub fn refresh(&mut self, snapshot: &Snapshot) {
        let root = snapshot.disk.volumes.iter().find(|volume| {
            volume
                .mount_points
                .iter()
                .any(|mount| &*mount.path == ROOT_MOUNT)
        });
        if let Some(capacity) = root.and_then(|volume| volume.capacity) {
            self.used_bytes = capacity.used.get();
            self.total_bytes = capacity.size.get();
        }
    }
}
