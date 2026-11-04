//! GPU histories, one set per device, and which device the GPU page shows.

use monitor_core::{DeviceKey, Percent};
use monitor_engine::Snapshot;
use monitor_engine::hardware::{EngineClass, GpuDevice, GpuKind, GpuPower};

use crate::app::live::{HISTORY_CAPACITY, push_sample};

/// The busiest-GPU engines the Overview shows.
pub const GPU_ENGINES: [&str; 3] = ["3D", "Compute", "Video"];
const OVERVIEW_ENGINES: [EngineClass; GPU_ENGINES.len()] = [
    EngineClass::Graphics,
    EngineClass::Compute,
    EngineClass::Video,
];

/// The engine overlay of the GPU page, in chart order.
pub const ENGINE_SERIES: [(EngineClass, &str); 6] = [
    (EngineClass::Graphics, "Graphics"),
    (EngineClass::Compute, "Compute"),
    (EngineClass::Copy, "Copy"),
    (EngineClass::Video, "Video decode"),
    (EngineClass::VideoEncode, "Video encode"),
    (EngineClass::VideoEnhance, "Enhance"),
];

/// One device's live charts.
#[derive(Debug)]
pub struct GpuHistory {
    pub key: DeviceKey,
    pub busy: Vec<f32>,
    pub engines: [Vec<f32>; ENGINE_SERIES.len()],
}

impl GpuHistory {
    fn new(key: DeviceKey) -> Self {
        let empty = || vec![f32::NAN; HISTORY_CAPACITY];
        Self {
            key,
            busy: empty(),
            engines: std::array::from_fn(|_| empty()),
        }
    }

    fn histories(&mut self) -> impl Iterator<Item = &mut Vec<f32>> {
        std::iter::once(&mut self.busy).chain(self.engines.iter_mut())
    }

    fn push(&mut self, device: &GpuDevice) {
        let metrics = &device.metrics;
        let asleep = metrics.power_state == GpuPower::Asleep;
        let idle_when_asleep = |value: Option<Percent>| match value {
            Some(percent) => percent.get(),
            None if asleep => 0.0,
            None => f32::NAN,
        };
        push_sample(&mut self.busy, idle_when_asleep(metrics.utilization));
        for (history, (class, _)) in self.engines.iter_mut().zip(ENGINE_SERIES) {
            push_sample(history, idle_when_asleep(metrics.engines.get(class)));
        }
    }
}

#[derive(Debug, Default)]
pub struct GpuReadings {
    pub busy: Vec<f32>,
    pub engines: [Vec<f32>; GPU_ENGINES.len()],
    pub vram_used_bytes: u64,
    pub vram_total_bytes: u64,
    pub discrete_asleep: bool,
    /// In the order of the snapshot's devices.
    pub devices: Vec<GpuHistory>,
    /// The device the GPU page shows, also sent to the engine as its interest.
    pub selected: Option<DeviceKey>,
    pub strained: bool,
}

/// The device to show: the chosen one while it exists, else the one driving the displays,
/// else the first.
pub fn selected_index(devices: &[GpuDevice], selected: Option<&DeviceKey>) -> Option<usize> {
    selected
        .and_then(|key| {
            devices
                .iter()
                .position(|device| &device.identity.key == key)
        })
        .or_else(|| devices.iter().position(|device| device.identity.boot_vga))
        .or_else(|| (!devices.is_empty()).then_some(0))
}

/// The GPU the single-GPU panels describe: the one driving the displays.
fn primary_gpu(snapshot: &Snapshot) -> Option<&GpuDevice> {
    let devices = &snapshot.gpu.devices;
    selected_index(devices, None).and_then(|index| devices.get(index))
}

impl GpuReadings {
    pub(super) fn histories(&mut self) -> impl Iterator<Item = &mut Vec<f32>> {
        std::iter::once(&mut self.busy)
            .chain(self.engines.iter_mut())
            .chain(self.devices.iter_mut().flat_map(GpuHistory::histories))
    }

    pub fn prepare(&mut self) {
        for history in self.histories() {
            history.clear();
            history.resize(HISTORY_CAPACITY, f32::NAN);
        }
    }

    /// Keeps one history per device, matched by PCI address, so a hotplug or a reorder
    /// never hands one GPU's past to another.
    fn sync_devices(&mut self, devices: &[GpuDevice]) {
        let unchanged = self.devices.len() == devices.len()
            && self
                .devices
                .iter()
                .zip(devices)
                .all(|(history, device)| history.key == device.identity.key);
        if unchanged {
            return;
        }
        let mut previous = std::mem::take(&mut self.devices);
        self.devices = devices
            .iter()
            .map(|device| {
                let key = &device.identity.key;
                match previous.iter().position(|history| &history.key == key) {
                    Some(index) => previous.swap_remove(index),
                    None => GpuHistory::new(key.clone()),
                }
            })
            .collect();
    }

    pub fn push(&mut self, snapshot: &Snapshot) {
        push_sample(&mut self.busy, snapshot.busiest_gpu().get());
        let shown = primary_gpu(snapshot);
        for (history, class) in self.engines.iter_mut().zip(OVERVIEW_ENGINES) {
            let busy = shown
                .and_then(|device| device.metrics.engines.get(class))
                .map_or(f32::NAN, Percent::get);
            push_sample(history, busy);
        }
        let devices = &snapshot.gpu.devices;
        self.sync_devices(devices);
        for (history, device) in self.devices.iter_mut().zip(devices) {
            history.push(device);
        }
    }

    pub fn push_gap(&mut self, snapshot: &Snapshot) {
        self.sync_devices(&snapshot.gpu.devices);
        for history in self.histories() {
            push_sample(history, f32::NAN);
        }
    }

    pub fn refresh(&mut self, snapshot: &Snapshot) {
        let video_memory = primary_gpu(snapshot).map(|device| device.metrics.memory);
        self.vram_used_bytes = video_memory
            .and_then(|memory| memory.vram_used)
            .map_or(0, |bytes| bytes.get());
        self.vram_total_bytes = video_memory
            .and_then(|memory| memory.vram_total)
            .map_or(0, |bytes| bytes.get());
        self.discrete_asleep = snapshot.gpu.devices.iter().any(|device| {
            device.identity.kind == GpuKind::Discrete
                && device.metrics.power_state == GpuPower::Asleep
        });
        self.strained = snapshot.strain.gpu.is_strained();
    }

    pub fn history(&self, key: &DeviceKey) -> Option<&GpuHistory> {
        self.devices.iter().find(|history| &history.key == key)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use monitor_engine::hardware::gpu::PciIdentity;
    use monitor_engine::hardware::{
        BackendKind, DriverSupport, GpuIdentity, GpuMetrics, GpuVendor,
    };

    use super::*;

    pub(crate) fn device(key: &str, kind: GpuKind, boot_vga: bool) -> GpuDevice {
        GpuDevice {
            identity: Arc::new(GpuIdentity {
                key: DeviceKey::new(key),
                card: "card0".into(),
                name: key.into(),
                vendor: GpuVendor::Amd,
                pci: PciIdentity::default(),
                driver: "amdgpu".into(),
                driver_version: None,
                vbios: None,
                kind,
                boot_vga,
                connected_displays: Arc::new([]),
                backend: BackendKind::Amd,
                support: DriverSupport::Full,
            }),
            metrics: GpuMetrics::default(),
        }
    }

    #[test]
    fn the_selection_falls_back_to_the_boot_gpu() {
        let devices = [
            device("0000:01:00.0", GpuKind::Discrete, false),
            device("0000:06:00.0", GpuKind::Integrated, true),
        ];
        assert_eq!(selected_index(&devices, None), Some(1));
        let chosen = DeviceKey::new("0000:01:00.0");
        assert_eq!(selected_index(&devices, Some(&chosen)), Some(0));
        let gone = DeviceKey::new("0000:09:00.0");
        assert_eq!(selected_index(&devices, Some(&gone)), Some(1));
        assert_eq!(selected_index(&[], Some(&chosen)), None);
    }

    #[test]
    fn engines_map_to_six_named_series() {
        let classes: Vec<_> = ENGINE_SERIES.iter().map(|(class, _)| *class).collect();
        assert!(!classes.contains(&EngineClass::Other));
        assert_eq!(ENGINE_SERIES[3].1, "Video decode");
        let mut metrics = GpuMetrics::default();
        metrics
            .engines
            .set(EngineClass::VideoEncode, Percent::new(30.0));
        let mut history = GpuHistory::new(DeviceKey::new("a"));
        history.push(&GpuDevice {
            metrics,
            ..device("a", GpuKind::Discrete, false)
        });
        assert_eq!(history.engines[4].last().copied(), Some(30.0));
        assert!(
            history.engines[0]
                .last()
                .is_some_and(|value| value.is_nan())
        );
    }

    #[test]
    fn asleep_gpus_chart_as_idle_and_histories_follow_their_device() {
        let mut readings = GpuReadings::default();
        let mut asleep = device("b", GpuKind::Discrete, false);
        asleep.metrics.power_state = GpuPower::Asleep;
        let mut snapshot = Snapshot::default();
        snapshot.gpu.devices = vec![device("a", GpuKind::Integrated, true), asleep];
        readings.push(&snapshot);
        let key = DeviceKey::new("b");
        assert_eq!(
            readings.history(&key).and_then(|h| h.busy.last().copied()),
            Some(0.0)
        );
        snapshot.gpu.devices.reverse();
        readings.push(&snapshot);
        assert_eq!(readings.devices[0].key, key);
        assert_eq!(readings.devices[0].busy.len(), HISTORY_CAPACITY);
    }
}
