//! The live readings the pages draw: plain data, refilled in place on every sample.

use std::sync::Arc;

use monitor_engine::Snapshot;

use crate::components::app_table::{AppCategory, RowMark, RowRef, TableRows};

pub use super::bridge::cpu::{CpuReadings, TYPICAL_TJMAX_CELSIUS, cpu_sensors};
pub use super::bridge::disk::DiskReadings;
#[cfg(test)]
pub(crate) use super::bridge::fixture;
pub use super::bridge::gpu::{
    ENGINE_SERIES, GPU_ENGINES, GpuHistory, GpuReadings, selected_index as selected_gpu_index,
};
pub use super::bridge::memory::{
    FREE_CATEGORY, MEMORY_CATEGORIES, MemoryReadings, USED_CATEGORIES,
};
pub use super::bridge::network::{InterfaceHistory, NetworkReadings};
pub use super::bridge::overview::{AcceleratorHistory, OverviewReadings};
pub use super::bridge::process_detail::{AppDetail, GpuUse, ProcessDetail, TableDetails};
pub use super::bridge::row_facts::{AppFacts, ENDED_GROUP_KEY, ProcessFacts};
use super::bridge::row_facts::{role_label, write_ended, write_roles};

/// Samples kept per series: 30 minutes at one per second.
pub const HISTORY_CAPACITY: usize = 30 * 60;

#[derive(Debug, Default)]
pub struct LiveSnapshot {
    pub cpu: CpuReadings,
    pub memory: MemoryReadings,
    pub gpu: GpuReadings,
    pub disk: DiskReadings,
    pub network: NetworkReadings,
    pub apps: AppList,
    pub overview: OverviewReadings,
    /// The newest engine tick, shared rather than copied: detail panels read it directly.
    pub tick: Arc<Snapshot>,
    /// Extra per-row figures, parallel to `apps`.
    pub details: TableDetails,
}

#[derive(Debug, Default)]
pub struct AppList {
    pub apps: Vec<AppEntry>,
}

/// One app row; the Ended recently group is a trailing entry whose category is `Ended`.
#[derive(Debug, Default)]
pub struct AppEntry {
    pub key: u64,
    pub name: Arc<str>,
    pub icon: &'static str,
    pub mark: RowMark,
    pub processes: Vec<ProcessEntry>,
    pub facts: AppFacts,
}

#[derive(Debug, Default)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: Arc<str>,
    pub user: Arc<str>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub threads: u32,
    pub mark: RowMark,
    pub facts: ProcessFacts,
}

impl AppEntry {
    pub fn cpu_percent(&self) -> f32 {
        self.processes
            .iter()
            .map(|process| process.cpu_percent)
            .sum()
    }

    pub fn memory_bytes(&self) -> u64 {
        self.processes
            .iter()
            .map(|process| process.memory_bytes)
            .sum()
    }

    pub fn threads(&self) -> u32 {
        self.processes.iter().map(|process| process.threads).sum()
    }
}

/// The newest `count` samples of `history`.
pub fn recent(history: &[f32], count: usize) -> &[f32] {
    &history[history.len().saturating_sub(count)..]
}

/// The newest sample, or NaN for an empty series.
pub fn latest(history: &[f32]) -> f32 {
    history.last().copied().unwrap_or(f32::NAN)
}

/// Drops the oldest sample and appends `value`, without reallocating.
pub fn push_sample(history: &mut [f32], value: f32) {
    if let Some(last) = history.len().checked_sub(1) {
        history.copy_within(1.., 0);
        history[last] = value;
    }
}

impl AppList {
    pub fn process(&self, app: usize, process: usize) -> Option<&ProcessEntry> {
        self.apps.get(app)?.processes.get(process)
    }

    pub fn app_by_key(&self, key: u64) -> Option<&AppEntry> {
        self.apps.iter().find(|app| app.key == key)
    }
}

impl TableRows for AppList {
    fn app_count(&self) -> usize {
        self.apps.len()
    }

    fn process_count(&self, app: usize) -> usize {
        self.apps.get(app).map_or(0, |app| app.processes.len())
    }

    fn app_key(&self, app: usize) -> u64 {
        self.apps.get(app).map_or(0, |app| app.key)
    }

    fn process_key(&self, app: usize, process: usize) -> u64 {
        self.process(app, process)
            .map_or(0, |process| u64::from(process.pid))
    }

    fn name(&self, row: RowRef) -> &str {
        match row {
            RowRef::App(app) => self.apps.get(app).map_or("", |app| &app.name),
            RowRef::Process(app, process) => self
                .process(app, process)
                .map_or("", |process| &process.name),
        }
    }

    fn icon(&self, app: usize) -> &'static str {
        self.apps.get(app).map_or("", |app| app.icon)
    }

    fn mark(&self, row: RowRef) -> RowMark {
        match row {
            RowRef::App(app) => self.apps.get(app).map_or(RowMark::None, |app| app.mark),
            RowRef::Process(app, process) => self
                .process(app, process)
                .map_or(RowMark::None, |process| process.mark),
        }
    }

    fn category(&self, app: usize) -> AppCategory {
        self.apps
            .get(app)
            .map_or(AppCategory::Mine, |app| app.facts.category)
    }

    fn detail(&self, row: RowRef, out: &mut String) {
        let Some(app) = self.apps.get(row.app()) else {
            return;
        };
        match row {
            RowRef::App(_) => write_roles(out, &app.facts.roles),
            RowRef::Process(_, process) => {
                let Some(process) = app.processes.get(process) else {
                    return;
                };
                if app.facts.category == AppCategory::Ended {
                    let offset = monitor_engine::utc_offset_seconds();
                    write_ended(out, &process.facts, offset);
                } else {
                    out.push_str(role_label(process.facts.role));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pushing_shifts_the_window_left() {
        let mut history = [1.0, 2.0, 3.0];
        push_sample(&mut history, 4.0);
        assert_eq!(history, [2.0, 3.0, 4.0]);
        assert_eq!(recent(&history, 2), &[3.0, 4.0]);
        assert_eq!(recent(&history, 10), &history);
    }
}
