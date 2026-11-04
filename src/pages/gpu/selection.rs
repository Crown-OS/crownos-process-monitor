use crownui::prelude::{Cx, Signal};
use monitor_engine::hardware::GpuDevice;

use crate::app::live::{LiveSnapshot, selected_gpu_index};

/// The device the page shows: an index into the tick's devices, kept in step with the live
/// readings' `gpu.selected` so the engine samples that GPU in detail.
#[derive(Clone, Copy)]
pub struct GpuChoice {
    pub index: Signal<usize>,
}

impl GpuChoice {
    pub fn new(cx: &mut Cx, live: Signal<LiveSnapshot>) -> Self {
        let initial = live.with_untracked(cx, |snapshot| {
            selected_gpu_index(&snapshot.tick.gpu.devices, snapshot.gpu.selected.as_ref())
                .unwrap_or_default()
        });
        let index = cx.signal(initial);
        cx.effect(move |runtime| {
            let chosen = index.get(runtime);
            let key = live.with_untracked(runtime, |snapshot| {
                let key = snapshot.tick.gpu.devices.get(chosen)?.identity.key.clone();
                (snapshot.gpu.selected.as_ref() != Some(&key)).then_some(key)
            });
            if let Some(key) = key {
                live.update(runtime, |snapshot| snapshot.gpu.selected = Some(key));
            }
        });
        Self { index }
    }

    /// Reads the chosen device of the live tick, subscribing to both.
    pub fn with<T>(
        self,
        runtime: &mut crownui::prelude::Runtime,
        live: Signal<LiveSnapshot>,
        read: impl FnOnce(Option<&GpuDevice>) -> T,
    ) -> T {
        let index = self.index.get(runtime);
        live.with(runtime, |snapshot| {
            read(snapshot.tick.gpu.devices.get(index))
        })
    }
}
