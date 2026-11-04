use monitor_core::{AppKey, Coverage, Pid, ProcessKey};

use super::row::{EndedProcess, ExternalUsage, ProcessRow};
use crate::apps::AppRow;

/// The process sweep of one tick, grouped into apps.
#[derive(Clone, Debug, Default)]
pub struct ProcessTable {
    /// The interval spans a suspend or is the first sample: rates are zero.
    pub gap: bool,
    /// Ascending by pid.
    pub processes: Vec<ProcessRow>,
    pub apps: Vec<AppRow>,
    /// Most recent first.
    pub ended: Vec<EndedProcess>,
    pub thread_count: u32,
    pub logical_cpus: u32,
    /// `OwnProcesses` when other users' I/O counters could not be read.
    pub io_coverage: Coverage,
    /// `OwnProcesses` when other users' PSS could not be read.
    pub memory_coverage: Coverage,
}

impl ProcessTable {
    pub fn index_of(&self, pid: Pid) -> Option<usize> {
        self.processes
            .binary_search_by_key(&pid, |row| row.key.pid)
            .ok()
    }

    pub fn process(&self, key: ProcessKey) -> Option<&ProcessRow> {
        self.index_of(key.pid)
            .map(|index| &self.processes[index])
            .filter(|row| row.key == key)
    }

    pub fn app_index(&self, key: &AppKey) -> Option<usize> {
        self.apps.iter().position(|app| app.key == *key)
    }

    /// The processes of the app at `app` (an index into `apps`).
    pub fn processes_of(&self, app: usize) -> impl Iterator<Item = &ProcessRow> {
        self.processes.iter().filter(move |row| row.app == app)
    }

    /// Replaces the external figures of every process and app with the sums
    /// of `usage`. Pids not in the table are ignored.
    pub fn apply_external(&mut self, usage: impl IntoIterator<Item = (Pid, ExternalUsage)>) {
        self.processes
            .iter_mut()
            .for_each(|row| row.external = ExternalUsage::default());
        self.apps
            .iter_mut()
            .for_each(|app| app.external = ExternalUsage::default());
        for (pid, figures) in usage {
            let Some(index) = self.index_of(pid) else {
                continue;
            };
            let row = &mut self.processes[index];
            row.external += figures;
            if let Some(app) = self.apps.get_mut(row.app) {
                app.external += figures;
            }
        }
    }
}
