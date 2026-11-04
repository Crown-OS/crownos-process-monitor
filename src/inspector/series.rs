use crownui::prelude::{Cx, Signal};
use monitor_core::ProcessKey;

use crate::app::AppState;
use crate::app::live::push_sample;
use crate::settings::CpuScale;

/// Two minutes at one sample per tick.
pub const SERIES_CAPACITY: usize = 120;

/// CPU percent and resident bytes of the inspected process, newest last, NaN before it was
/// seen or after it exited.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessSeries {
    pub key: Option<ProcessKey>,
    pub cpu: Vec<f32>,
    pub memory: Vec<f32>,
}

impl Default for ProcessSeries {
    fn default() -> Self {
        Self {
            key: None,
            cpu: vec![f32::NAN; SERIES_CAPACITY],
            memory: vec![f32::NAN; SERIES_CAPACITY],
        }
    }
}

impl ProcessSeries {
    fn restart(&mut self, key: Option<ProcessKey>) {
        self.key = key;
        self.cpu.fill(f32::NAN);
        self.memory.fill(f32::NAN);
    }
}

/// Appends one sample per tick while the calling scope (the open drawer) lives.
pub fn follow(
    cx: &mut Cx,
    state: AppState,
    target: Signal<Option<ProcessKey>>,
    series: Signal<ProcessSeries>,
) {
    let settings = state.settings;
    let live = state.live;
    cx.effect(move |runtime| {
        let Some(key) = target.get(runtime) else {
            return;
        };
        let per_core = settings.current(runtime).cpu_scale == CpuScale::PerCore;
        let sample = live.with(runtime, |snapshot| {
            snapshot.tick.processes.process(key).map(|row| {
                let cpu = if per_core { row.cpu } else { row.cpu_machine };
                (cpu.get(), row.resident.get() as f32)
            })
        });
        series.update(runtime, |series| {
            if series.key != Some(key) {
                series.restart(Some(key));
            }
            let (cpu, memory) = sample.unwrap_or((f32::NAN, f32::NAN));
            push_sample(&mut series.cpu, cpu);
            push_sample(&mut series.memory, memory);
        });
    });
}
