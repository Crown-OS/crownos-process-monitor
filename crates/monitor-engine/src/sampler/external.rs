use std::collections::HashMap;

use monitor_core::{Bytes, ProcessKey};
use monitor_system::ExternalUsage;

use crate::Snapshot;

/// TCP bytes per process since the engine first saw it, summed over traffic
/// passes; keyed by [`ProcessKey`] so a reused pid starts from zero.
#[derive(Debug, Default)]
pub(crate) struct TrafficTotals {
    passes: u64,
    totals: HashMap<ProcessKey, (Bytes, Bytes)>,
}

impl TrafficTotals {
    /// Adds a fresh traffic pass and forgets processes that ended.
    pub fn update(&mut self, snapshot: &Snapshot) {
        let network = &snapshot.network;
        if network.traffic_passes == self.passes {
            return;
        }
        self.passes = network.traffic_passes;
        let table = &snapshot.processes;
        for traffic in &network.process_traffic {
            let Some(index) = table.index_of(traffic.pid) else {
                continue;
            };
            let (received, transmitted) =
                self.totals.entry(table.processes[index].key).or_default();
            *received += traffic.received;
            *transmitted += traffic.transmitted;
        }
        self.totals.retain(|key, _| table.process(*key).is_some());
    }
}

/// Attributes this tick's per-process GPU and TCP figures to processes and
/// their apps.
pub(crate) fn join_external(snapshot: &mut Snapshot, traffic_totals: &TrafficTotals) {
    let gpu = snapshot.gpu.processes.iter().map(|usage| {
        (
            usage.pid,
            ExternalUsage {
                gpu: usage.utilization,
                gpu_memory: usage.memory + usage.shared_memory,
                ..ExternalUsage::default()
            },
        )
    });
    let network = snapshot.network.process_traffic.iter().map(|traffic| {
        (
            traffic.pid,
            ExternalUsage {
                network_receive: traffic.receive,
                network_transmit: traffic.transmit,
                ..ExternalUsage::default()
            },
        )
    });
    let totals = traffic_totals
        .totals
        .iter()
        .map(|(key, &(received, transmitted))| {
            (
                key.pid,
                ExternalUsage {
                    network_received_total: received,
                    network_transmitted_total: transmitted,
                    ..ExternalUsage::default()
                },
            )
        });
    snapshot
        .processes
        .apply_external(gpu.chain(network).chain(totals));
}

#[cfg(test)]
mod tests {
    use monitor_core::{BytesPerSecond, Pid};
    use monitor_io::ProcessTraffic;
    use monitor_system::ProcessRow;

    use super::*;

    fn row(pid: u32, start_ticks: u64) -> ProcessRow {
        ProcessRow {
            key: ProcessKey {
                pid: Pid::new(pid),
                start_ticks,
            },
            parent: Pid::new(1),
            uid: monitor_core::Uid::new(1000),
            name: "curl".into(),
            command_line: "curl".into(),
            executable: None,
            app: 0,
            role: monitor_system::ProcessRole::Main,
            state: monitor_system::ProcessState::Running,
            threads: 1,
            nice: 0,
            cpu: monitor_core::Percent::ZERO,
            cpu_machine: monitor_core::Percent::ZERO,
            cpu_time: std::time::Duration::ZERO,
            resident: Bytes::ZERO,
            proportional: None,
            unique: None,
            swap: None,
            peak_resident: None,
            minor_faults: 0,
            major_faults: 0,
            disk_read: BytesPerSecond::ZERO,
            disk_write: BytesPerSecond::ZERO,
            read_since_start: None,
            written_since_start: None,
            external: ExternalUsage::default(),
            started_unix: 0,
            is_new: false,
            kernel_thread: false,
            coverage: monitor_core::Coverage::Full,
        }
    }

    fn pass(snapshot: &mut Snapshot, received: u64) {
        snapshot.network.traffic_passes += 1;
        snapshot.network.process_traffic = vec![ProcessTraffic {
            pid: Pid::new(42),
            receive: BytesPerSecond::new(received as f64),
            transmit: BytesPerSecond::ZERO,
            received: Bytes::new(received),
            transmitted: Bytes::ZERO,
        }];
    }

    #[test]
    fn sums_fresh_passes_only_and_restarts_for_a_reused_pid() {
        let mut snapshot = Snapshot::default();
        snapshot.processes.processes = vec![row(42, 7)];
        let mut totals = TrafficTotals::default();
        pass(&mut snapshot, 100);
        totals.update(&snapshot);
        totals.update(&snapshot);
        pass(&mut snapshot, 50);
        totals.update(&snapshot);
        join_external(&mut snapshot, &totals);
        assert_eq!(
            snapshot.processes.processes[0]
                .external
                .network_received_total,
            Bytes::new(150)
        );

        snapshot.processes.processes = vec![row(42, 9)];
        pass(&mut snapshot, 10);
        totals.update(&snapshot);
        join_external(&mut snapshot, &totals);
        assert_eq!(
            snapshot.processes.processes[0]
                .external
                .network_received_total,
            Bytes::new(10)
        );
    }
}
