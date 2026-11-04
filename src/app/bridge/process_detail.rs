//! The per-process and per-app figures the CPU, Memory and GPU tables show beyond the shared
//! columns, copied from the process table after each refill.

use monitor_core::{Page, Pid};
use monitor_engine::Snapshot;
use monitor_engine::hardware::{GpuKind, GpuSnapshot};
use monitor_engine::system::{MemoryMethod, ProcessRow};

use crate::app::live::{AppList, LiveSnapshot};
use crate::components::app_table::{AppCategory, RowMark, RowRef, TableRows};

/// Which kinds of GPU a process has open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GpuUse {
    #[default]
    None,
    Integrated,
    Discrete,
    Both,
}

impl GpuUse {
    #[must_use]
    pub const fn with(self, kind: GpuKind) -> Self {
        match (self, kind) {
            (Self::None | Self::Integrated, GpuKind::Integrated) => Self::Integrated,
            (Self::None | Self::Discrete, GpuKind::Discrete) => Self::Discrete,
            _ => Self::Both,
        }
    }

    #[must_use]
    pub const fn combined(self, other: Self) -> Self {
        match (self, other) {
            (Self::None, used) | (used, Self::None) => used,
            (Self::Integrated, Self::Integrated) => Self::Integrated,
            (Self::Discrete, Self::Discrete) => Self::Discrete,
            _ => Self::Both,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Integrated => "Integrated",
            Self::Discrete => "Discrete",
            Self::Both => "Both",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProcessDetail {
    pub cpu_seconds: u64,
    pub resident: u64,
    pub proportional: Option<u64>,
    pub unique: Option<u64>,
    pub swap: Option<u64>,
    pub peak_resident: Option<u64>,
    pub gpu_memory: u64,
    pub gpu: GpuUse,
}

impl From<&ProcessRow> for ProcessDetail {
    fn from(row: &ProcessRow) -> Self {
        Self {
            cpu_seconds: row.cpu_time.as_secs(),
            resident: row.resident.get(),
            proportional: row.proportional.map(|bytes| bytes.get()),
            unique: row.unique.map(|bytes| bytes.get()),
            swap: row.swap.map(|bytes| bytes.get()),
            peak_resident: row.peak_resident.map(|bytes| bytes.get()),
            gpu_memory: row.external.gpu_memory.get(),
            gpu: GpuUse::None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppDetail {
    /// Measured by `memory_method`, so exact for apps that own their cgroup.
    pub memory: u64,
    pub memory_method: MemoryMethod,
}

/// Per-row figures parallel to [`AppList`]: `apps[i]` describes `list.apps[i]` and
/// `processes[i][j]` its `j`th process. Inner vectors are reused across ticks.
#[derive(Debug, Default)]
pub struct TableDetails {
    pub apps: Vec<AppDetail>,
    pub processes: Vec<Vec<ProcessDetail>>,
}

impl TableDetails {
    pub fn app(&self, app: usize) -> AppDetail {
        self.apps.get(app).copied().unwrap_or_default()
    }

    pub fn process(&self, app: usize, process: usize) -> ProcessDetail {
        self.processes
            .get(app)
            .and_then(|processes| processes.get(process))
            .copied()
            .unwrap_or_default()
    }

    /// Refills from a freshly refilled `list`: apps by position, live processes by pid (the
    /// Ended recently group gets none). Which GPU each process uses is joined only while the
    /// GPU page is shown.
    pub fn refill(&mut self, list: &AppList, snapshot: &Snapshot) {
        let table = &snapshot.processes;
        let join_gpus = snapshot.interest.page == Page::Gpu;
        self.apps.clear();
        self.apps.extend(
            table
                .apps
                .iter()
                .take(list.apps.len())
                .map(|app| AppDetail {
                    memory: app.memory.get(),
                    memory_method: app.memory_method,
                }),
        );
        self.processes.truncate(list.apps.len());
        self.processes.resize_with(list.apps.len(), Vec::new);
        for (details, entry) in self.processes.iter_mut().zip(&list.apps) {
            details.clear();
            if entry.facts.category == AppCategory::Ended {
                continue;
            }
            details.extend(entry.processes.iter().map(|process| {
                let pid = Pid::new(process.pid);
                let row = table
                    .index_of(pid)
                    .and_then(|index| table.processes.get(index));
                let mut detail = row.map(ProcessDetail::from).unwrap_or_default();
                if join_gpus {
                    detail.gpu = gpu_use(&snapshot.gpu, pid);
                }
                detail
            }));
        }
    }
}

/// Tables over the whole live snapshot see the app list's rows and can also read
/// [`TableDetails`] and the latest tick.
impl TableRows for LiveSnapshot {
    fn app_count(&self) -> usize {
        self.apps.app_count()
    }

    fn process_count(&self, app: usize) -> usize {
        self.apps.process_count(app)
    }

    fn app_key(&self, app: usize) -> u64 {
        self.apps.app_key(app)
    }

    fn process_key(&self, app: usize, process: usize) -> u64 {
        self.apps.process_key(app, process)
    }

    fn name(&self, row: RowRef) -> &str {
        self.apps.name(row)
    }

    fn icon(&self, app: usize) -> &'static str {
        self.apps.icon(app)
    }

    fn mark(&self, row: RowRef) -> RowMark {
        self.apps.mark(row)
    }

    fn category(&self, app: usize) -> AppCategory {
        self.apps.category(app)
    }

    fn detail(&self, row: RowRef, out: &mut String) {
        self.apps.detail(row, out);
    }
}

fn gpu_use(gpu: &GpuSnapshot, pid: Pid) -> GpuUse {
    gpu.processes
        .iter()
        .filter(|usage| usage.pid == pid)
        .filter_map(|usage| gpu.device(&usage.device))
        .fold(GpuUse::None, |used, device| used.with(device.identity.kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_use_combines_kinds() {
        let used = GpuUse::None.with(GpuKind::Integrated);
        assert_eq!(used, GpuUse::Integrated);
        assert_eq!(used.with(GpuKind::Integrated), GpuUse::Integrated);
        assert_eq!(used.with(GpuKind::Discrete), GpuUse::Both);
        assert_eq!(GpuUse::Discrete.with(GpuKind::Integrated), GpuUse::Both);
        assert_eq!(GpuUse::None.combined(GpuUse::Discrete), GpuUse::Discrete);
        assert_eq!(GpuUse::Integrated.combined(GpuUse::Discrete), GpuUse::Both);
    }
}
