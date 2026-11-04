use crownui::prelude::AnyView;
use monitor_core::Pid;
use monitor_engine::hardware::{EngineClass, GpuSnapshot};

use crate::inspector::fields::{Fields, section};
use crate::util::format::{bytes, percent};

/// Engine time and memory on every GPU the process has open; nothing when it uses none.
pub fn view(gpu: &GpuSnapshot, pid: Pid) -> Option<AnyView> {
    let mut rows = Vec::new();
    for usage in gpu.processes.iter().filter(|usage| usage.pid == pid) {
        let name = gpu.device(&usage.device).map_or_else(
            || usage.device.as_str().to_owned(),
            |device| device.identity.name.to_string(),
        );
        let engines: Vec<String> = EngineClass::ALL
            .iter()
            .filter_map(|class| {
                let busy = usage.engines.get(*class)?;
                (busy.get() > 0.0).then(|| format!("{} {}", class.title(), percent(busy.get())))
            })
            .collect();
        let mut fields = Fields::default();
        fields
            .add("Device", name)
            .add("Busiest engine", percent(usage.utilization.get()))
            .add("Engines", engines.join(" · "))
            .add("Video memory", bytes(usage.memory.get()))
            .add("Shared memory", bytes(usage.shared_memory.get()));
        rows.extend(fields.into_views());
    }
    (!rows.is_empty()).then(|| section("GPU", rows))
}
