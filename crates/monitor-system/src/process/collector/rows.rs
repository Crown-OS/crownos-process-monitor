use monitor_core::{Bytes, Coverage, Percent, Pid};

use super::{NEW_PROCESS_SECONDS, ProcessCollector};
use crate::apps::MemberFigures;
use crate::process::entry::ProcessEntry;
use crate::process::row::{ExternalUsage, ProcessRow, ProcessState};
use crate::process::table::ProcessTable;
use crate::util::clock;

impl ProcessCollector {
    pub(super) fn build(&mut self, seconds: Option<f64>, now_unix: u64, output: &mut ProcessTable) {
        let cycle = self.pss_cycle;
        let fresh_rollup =
            |entry: &ProcessEntry| entry.rollup.filter(|_| entry.rollup_cycle + 1 >= cycle);
        self.registry.begin();
        self.own_pids.clear();
        let mut threads = 0;
        let mut io_partial = false;
        let mut memory_partial = false;
        for &slot in &self.row_slots {
            let Some(entry) = self.entries[slot].as_ref() else {
                continue;
            };
            threads += entry.stat.threads;
            if entry.uid == self.own_uid && !entry.stat.is_kernel_thread() {
                self.own_pids.push(entry.key.pid);
            }
            io_partial |= entry.io_restricted();
            memory_partial |= entry.memory_restricted();
            let Some(app) = entry.app else {
                continue;
            };
            let (disk_read, disk_write) = entry.io_rates();
            self.registry.add(
                app,
                &MemberFigures {
                    cpu: entry.cpu,
                    resident: entry.resident,
                    proportional: fresh_rollup(entry).map(|rollup| rollup.proportional),
                    disk_read,
                    disk_write,
                    threads: entry.stat.threads,
                    role: entry.role,
                    owned_scope: entry.scope_path.as_ref(),
                    partial: entry.is_restricted(),
                    started_unix: entry.started_unix,
                },
            );
        }
        self.registry
            .finish(seconds, self.logical_cpus, &mut output.apps);

        output.processes.clear();
        for &slot in &self.row_slots {
            let Some(entry) = self.entries[slot].as_ref() else {
                continue;
            };
            let app = entry.app.map_or(0, |app| self.registry.row_of(app));
            output.processes.push(self.row(entry, app, now_unix));
        }
        self.ended.copy_into(&mut output.ended);
        output.gap = seconds.is_none();
        output.thread_count = threads;
        output.logical_cpus = self.logical_cpus;
        output.io_coverage = if io_partial {
            Coverage::OwnProcesses
        } else {
            Coverage::Full
        };
        output.memory_coverage = if memory_partial {
            Coverage::OwnProcesses
        } else {
            Coverage::Full
        };
    }

    fn row(&self, entry: &ProcessEntry, app: usize, now_unix: u64) -> ProcessRow {
        let (disk_read, disk_write) = entry.io_rates();
        let io_totals = entry.io_totals();
        let stat = &entry.stat;
        ProcessRow {
            key: entry.key,
            parent: Pid::new(stat.parent),
            uid: entry.uid,
            name: entry.comm.clone(),
            command_line: entry.command_line.clone(),
            executable: entry.executable.clone(),
            app,
            role: entry.role,
            state: ProcessState::from_code(stat.state),
            threads: stat.threads,
            nice: i32::try_from(stat.nice).unwrap_or(0),
            cpu: entry.cpu,
            cpu_machine: Percent::new(entry.cpu.get() / self.logical_cpus.max(1) as f32),
            cpu_time: clock::ticks_to_duration(stat.cpu_ticks(), self.ticks_per_second),
            resident: entry.resident,
            proportional: entry.rollup.map(|rollup| rollup.proportional),
            unique: entry.rollup.map(|rollup| rollup.unique),
            swap: entry
                .rollup
                .map(|rollup| rollup.swap)
                .or(entry.status.map(|status| status.swap)),
            peak_resident: entry
                .status
                .map(|status| status.peak)
                .filter(|peak| *peak > Bytes::ZERO),
            minor_faults: stat.minor_faults,
            major_faults: stat.major_faults,
            disk_read,
            disk_write,
            read_since_start: io_totals.map(|(read, _)| read),
            written_since_start: io_totals.map(|(_, written)| written),
            external: ExternalUsage::default(),
            started_unix: entry.started_unix,
            is_new: now_unix.saturating_sub(entry.started_unix) < NEW_PROCESS_SECONDS,
            kernel_thread: stat.is_kernel_thread(),
            coverage: if entry.is_restricted() {
                Coverage::OwnProcesses
            } else {
                Coverage::Full
            },
        }
    }
}
