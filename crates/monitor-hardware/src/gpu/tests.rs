use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use monitor_core::util::procfs::ProcRoot;
use monitor_core::{Bytes, Celsius, Elapsed, FdIndex, FdScope, Percent, Pid};

use std::sync::Arc;

use monitor_core::{Collector, Interest, SampleContext, Tick};

use super::clients::ClientTracker;
use super::paths::SystemPaths;
use super::*;

struct FakeSysfs {
    root: PathBuf,
}

impl FakeSysfs {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("monitor-hardware-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        Self { root }
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            assert!(fs::create_dir_all(parent).is_ok());
        }
        assert!(fs::write(path, content).is_ok());
    }

    fn link(&self, relative: &str, target: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            assert!(fs::create_dir_all(parent).is_ok());
        }
        assert!(std::os::unix::fs::symlink(self.root.join(target), path).is_ok());
    }

    fn paths(&self) -> SystemPaths {
        SystemPaths {
            sysfs: self.root.join("sys"),
            pci_ids: self.root.join("pci.ids"),
            amdgpu_ids: self.root.join("amdgpu.ids"),
            kernel_release: self.root.join("osrelease"),
        }
    }
}

impl Drop for FakeSysfs {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn sample_once(collector: &mut GpuCollector, fds: &FdIndex) -> GpuSnapshot {
    let interest = Interest::default();
    let context = SampleContext {
        tick: Tick::now(),
        elapsed: Elapsed::default(),
        interest: &interest,
        fds,
    };
    let mut snapshot = GpuSnapshot::default();
    assert!(collector.sample(&context, &mut snapshot).is_ok());
    snapshot
}

const DGPU: &str = "sys/devices/pci0000:00/0000:00:01.1/0000:03:00.0";

fn fake_amd_dgpu(sysfs: &FakeSysfs, runtime_status: &str) {
    sysfs.write(&format!("{DGPU}/vendor"), "0x1002\n");
    sysfs.write(&format!("{DGPU}/device"), "0x73bf\n");
    sysfs.write(&format!("{DGPU}/revision"), "0xc1\n");
    sysfs.write(&format!("{DGPU}/boot_vga"), "0\n");
    sysfs.write(&format!("{DGPU}/power/runtime_status"), runtime_status);
    sysfs.write(&format!("{DGPU}/gpu_busy_percent"), "77\n");
    sysfs.write(&format!("{DGPU}/mem_info_vram_used"), "1048576\n");
    sysfs.write(&format!("{DGPU}/mem_info_vram_total"), "17163091968\n");
    sysfs.write(&format!("{DGPU}/hwmon/hwmon4/temp1_input"), "51000\n");
    sysfs.write(&format!("{DGPU}/hwmon/hwmon4/temp1_label"), "edge\n");
    sysfs.write("sys/bus/pci/drivers/amdgpu/.keep", "");
    sysfs.link(&format!("{DGPU}/driver"), "sys/bus/pci/drivers/amdgpu");
    sysfs.link("sys/class/drm/card0/device", DGPU);
    sysfs.write("sys/class/drm/card0-DP-1/status", "connected\n");
    sysfs.write("sys/class/drm/card0-HDMI-A-1/status", "disconnected\n");
}

#[test]
fn asleep_device_is_never_queried() {
    let sysfs = FakeSysfs::new("wake-guard");
    fake_amd_dgpu(&sysfs, "suspended\n");
    let mut collector = GpuCollector::with_paths(sysfs.paths());
    let fds = FdIndex::default();

    let asleep = sample_once(&mut collector, &fds);
    let [device] = asleep.devices.as_slice() else {
        panic!("expected one GPU, found {}", asleep.devices.len());
    };
    assert_eq!(device.identity.key.as_str(), "0000:03:00.0");
    assert_eq!(device.identity.vendor, GpuVendor::Amd);
    assert_eq!(device.identity.kind, GpuKind::Discrete);
    assert_eq!(device.identity.backend, BackendKind::Amd);
    assert_eq!(
        &*device.identity.connected_displays,
        [Arc::<str>::from("DP-1")]
    );
    assert_eq!(device.metrics.power_state, GpuPower::Asleep);
    assert_eq!(device.metrics.utilization, None);
    assert_eq!(device.metrics.memory.vram_used, None);
    assert_eq!(device.metrics.temperatures.edge, None);

    sysfs.write(&format!("{DGPU}/power/runtime_status"), "active\n");
    let awake = sample_once(&mut collector, &fds);
    let metrics = awake.devices[0].metrics;
    assert_eq!(metrics.power_state, GpuPower::Active);
    assert_eq!(metrics.utilization, Some(Percent::new(77.0)));
    assert_eq!(metrics.memory.vram_used, Some(Bytes::new(1_048_576)));
    assert_eq!(metrics.temperatures.edge, Some(Celsius::new(51.0)));
}

#[test]
fn minimized_ticks_carry_device_metrics_until_the_history_pass() {
    let sysfs = FakeSysfs::new("minimized");
    fake_amd_dgpu(&sysfs, "active\n");
    let mut collector = GpuCollector::with_paths(sysfs.paths());
    let fds = FdIndex::default();
    let minimized = Interest {
        window: monitor_core::WindowState::Minimized,
        ..Interest::default()
    };
    let mut output = GpuSnapshot::default();
    let mut sample = |collector: &mut GpuCollector, interest: &Interest| {
        let context = SampleContext {
            tick: Tick::now(),
            elapsed: Elapsed::default(),
            interest,
            fds: &fds,
        };
        assert!(collector.sample(&context, &mut output).is_ok());
        output.devices[0].metrics
    };
    let first = sample(&mut collector, &minimized);
    assert_eq!(first.utilization, Some(Percent::new(77.0)));
    assert_eq!(
        first.pcie,
        PcieLink::default(),
        "no PCIe reads while minimized"
    );

    sysfs.write(&format!("{DGPU}/gpu_busy_percent"), "12\n");
    let carried = sample(&mut collector, &minimized);
    assert_eq!(carried, first, "within 15 s the last figures are carried");
    assert!(collector.device_pass_due(&minimized, true, Tick::now().monotonic()));
    let later = Tick::now().monotonic() + MINIMIZED_PROCESS_INTERVAL;
    assert!(collector.device_pass_due(&minimized, false, later));

    let visible = sample(&mut collector, &Interest::default());
    assert_eq!(visible.utilization, Some(Percent::new(12.0)));
}

#[test]
fn drm_scope_is_due_for_history() {
    let collector = GpuCollector::with_paths(FakeSysfs::new("due").paths());
    let hidden = Interest {
        page: Page::Cpu,
        ..Interest::default()
    };
    assert!(
        collector.drm_fds_due(&hidden, &Tick::now()),
        "first pass is always due"
    );
    let gpu_page = Interest {
        page: Page::Gpu,
        ..Interest::default()
    };
    assert!(wants_drm_fds(&gpu_page));
    assert!(!wants_drm_fds(&hidden));
}

fn own_drm_fds() -> FdIndex {
    let mut index = FdIndex::default();
    let Ok(mut root) = ProcRoot::open() else {
        return index;
    };
    let mut pids: Vec<Pid> = Vec::new();
    if root.list_pids(&mut pids).is_ok() {
        index.refresh(
            &root,
            &pids,
            FdScope {
                sockets: false,
                drm: true,
            },
        );
    }
    index
}

fn print_runtime_status(key: &str) {
    let status = fs::read_to_string(
        Path::new("/sys/bus/pci/devices")
            .join(key)
            .join("power/runtime_status"),
    );
    println!(
        "{key} runtime_status: {}",
        status.unwrap_or_default().trim()
    );
}

#[test]
fn live_discovers_hybrid_graphics() {
    let mut collector = GpuCollector::new();
    let fds = own_drm_fds();
    let _ = sample_once(&mut collector, &fds);
    std::thread::sleep(Duration::from_millis(300));
    let snapshot = sample_once(&mut collector, &fds);
    for device in &snapshot.devices {
        let identity = &device.identity;
        print_runtime_status(identity.key.as_str());
        println!(
            "{} {} {:?} {:?} {:?} driver={} {:?} vbios={:?} displays={:?} boot_vga={}",
            identity.card,
            identity.key,
            identity.name,
            identity.vendor,
            identity.kind,
            identity.driver,
            identity.driver_version,
            identity.vbios,
            identity.connected_displays,
            identity.boot_vga
        );
        println!("  {:?}", device.metrics);
    }
    println!(
        "processes: {} coverage {:?}",
        snapshot.processes.len(),
        snapshot.process_coverage
    );
    for process in snapshot.processes.iter().take(8) {
        println!("  {process:?}");
    }
    if let Some(amd) = snapshot
        .devices
        .iter()
        .find(|device| device.identity.key.as_str() == "0000:06:00.0")
    {
        assert_eq!(amd.identity.vendor, GpuVendor::Amd);
        assert_eq!(amd.identity.kind, GpuKind::Integrated);
        assert!(
            amd.identity.name.contains("780M"),
            "name {}",
            amd.identity.name
        );
        assert!(amd.metrics.memory.vram_total.is_some());
        assert!(amd.metrics.temperatures.edge.is_some());
    }
    if let Some(nvidia) = snapshot
        .devices
        .iter()
        .find(|device| device.identity.key.as_str() == "0000:01:00.0")
    {
        assert_eq!(nvidia.identity.vendor, GpuVendor::Nvidia);
        assert_eq!(nvidia.identity.kind, GpuKind::Discrete);
        if nvidia.metrics.power_state == GpuPower::Active
            && nvidia.identity.support == DriverSupport::Full
        {
            assert!(
                nvidia.identity.name.starts_with("NVIDIA"),
                "name {}",
                nvidia.identity.name
            );
            assert!(
                nvidia.metrics.memory.vram_total.is_some(),
                "NVML memory missing"
            );
        }
    }
}

#[test]
fn live_measures_sample_cost() {
    const SAMPLES: u32 = 20;
    let mut collector = GpuCollector::new();
    let fds = own_drm_fds();
    let _ = sample_once(&mut collector, &fds);
    let started = Instant::now();
    for _ in 0..SAMPLES {
        let _ = sample_once(&mut collector, &fds);
    }
    let per_sample = started.elapsed() / SAMPLES;
    println!(
        "GpuCollector sample with per-process pass over {} DRM fds: {per_sample:?}",
        fds.drm_fds().len()
    );
    let without = FdIndex::default();
    let started = Instant::now();
    for _ in 0..SAMPLES {
        let _ = sample_once(&mut collector, &without);
    }
    println!(
        "GpuCollector sample, devices only: {:?}",
        started.elapsed() / SAMPLES
    );
}

#[test]
fn live_measures_fdinfo_scan_cost() {
    const SAMPLES: u32 = 20;
    let collector = GpuCollector::new();
    let fds = own_drm_fds();
    let mut tracker = ClientTracker::default();
    let (mut usage, mut totals) = (Vec::new(), Vec::new());
    let started = Instant::now();
    for _ in 0..SAMPLES {
        tracker.scan(
            fds.drm_fds(),
            &collector.identities,
            Tick::now().monotonic(),
            &mut usage,
            &mut totals,
        );
    }
    println!(
        "fdinfo scan of {} fds: {:?}",
        fds.drm_fds().len(),
        started.elapsed() / SAMPLES
    );
}
