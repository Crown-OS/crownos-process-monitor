//! Per-client GPU usage from DRM fdinfo, deduplicated by
//! `(drm-pdev, drm-client-id)` and turned into rates between passes.

use std::collections::HashMap;
use std::fmt::Write;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::Duration;

use monitor_core::util::sysfs;
use monitor_core::{Bytes, DrmFd, Percent, Pid};
use rustix::fs::{Mode, OFlags};

use super::engine::{EngineClass, EngineUsage};
use super::fdinfo::{self, DrmClient, EngineCounter, EngineCounters};
use super::snapshot::{GpuIdentity, GpuProcessUsage};

const PERCENT: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ClientKey {
    device: usize,
    client_id: u64,
}

#[derive(Clone, Copy, Debug)]
struct ClientState {
    engines: EngineCounters,
    observed_at: Duration,
    pass: u64,
}

/// fdinfo sums for one device over the clients that could be read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct DeviceTotals {
    pub(crate) engines: EngineUsage,
    pub(crate) local_memory: Bytes,
    pub(crate) shared_memory: Bytes,
    pub(crate) clients: u32,
}

#[derive(Debug)]
pub(crate) struct ClientTracker {
    proc_directory: Option<OwnedFd>,
    states: HashMap<ClientKey, ClientState>,
    pass: u64,
    path: String,
    buffer: Vec<u8>,
}

impl Default for ClientTracker {
    fn default() -> Self {
        let proc_directory = rustix::fs::open(
            "/proc",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .ok();
        Self {
            proc_directory,
            states: HashMap::new(),
            pass: 0,
            path: String::with_capacity(32),
            buffer: Vec::new(),
        }
    }
}

impl ClientTracker {
    /// Forgets every client, e.g. after the device list changed.
    pub(crate) fn reset(&mut self) {
        self.states.clear();
    }

    /// Reads the fdinfo of every DRM descriptor and refills `usage` (one entry
    /// per process and device) and `totals` (one entry per device).
    pub(crate) fn scan(
        &mut self,
        fds: &[DrmFd],
        devices: &[Arc<GpuIdentity>],
        now: Duration,
        usage: &mut Vec<GpuProcessUsage>,
        totals: &mut Vec<DeviceTotals>,
    ) {
        self.begin(devices, usage, totals);
        let Some(proc_directory) = self.proc_directory.take() else {
            return;
        };
        let mut buffer = std::mem::take(&mut self.buffer);
        for drm_fd in fds {
            self.path.clear();
            if write!(self.path, "{}/fdinfo/{}", drm_fd.pid, drm_fd.fd).is_err() {
                continue;
            }
            if let Ok(content) = sysfs::read_at(&proc_directory, &self.path, &mut buffer) {
                self.observe(drm_fd.pid, content, devices, now, usage, totals);
            }
        }
        self.buffer = buffer;
        self.proc_directory = Some(proc_directory);
        self.finish(usage);
    }

    /// Like [`ClientTracker::scan`] for fdinfo texts read elsewhere (the
    /// privileged helper reads every user's).
    pub(crate) fn scan_texts<'a>(
        &mut self,
        clients: impl IntoIterator<Item = (Pid, &'a [u8])>,
        devices: &[Arc<GpuIdentity>],
        now: Duration,
        usage: &mut Vec<GpuProcessUsage>,
        totals: &mut Vec<DeviceTotals>,
    ) {
        self.begin(devices, usage, totals);
        for (pid, content) in clients {
            self.observe(pid, content, devices, now, usage, totals);
        }
        self.finish(usage);
    }

    fn begin(
        &mut self,
        devices: &[Arc<GpuIdentity>],
        usage: &mut Vec<GpuProcessUsage>,
        totals: &mut Vec<DeviceTotals>,
    ) {
        usage.clear();
        totals.clear();
        totals.resize(devices.len(), DeviceTotals::default());
        self.pass += 1;
    }

    fn observe(
        &mut self,
        pid: Pid,
        content: &[u8],
        devices: &[Arc<GpuIdentity>],
        now: Duration,
        usage: &mut Vec<GpuProcessUsage>,
        totals: &mut [DeviceTotals],
    ) {
        let client = fdinfo::parse(content);
        let Some(device) = resolve_device(&client, devices) else {
            return;
        };
        let Some(client_id) = client.client_id else {
            return;
        };
        if !client.has_usage() {
            return;
        }
        let key = ClientKey { device, client_id };
        let previous = self.states.get(&key).copied();
        if previous.is_some_and(|state| state.pass == self.pass) {
            return;
        }
        let engines = previous.map_or_else(
            || first_observation(&client.engines),
            |state| {
                rates(
                    &state.engines,
                    &client.engines,
                    now.saturating_sub(state.observed_at),
                )
            },
        );
        self.states.insert(
            key,
            ClientState {
                engines: client.engines,
                observed_at: now,
                pass: self.pass,
            },
        );
        let local_memory = Bytes::new(client.local_memory());
        let shared_memory = Bytes::new(client.shared_memory());
        let total = &mut totals[device];
        total.engines.merge(&engines);
        total.local_memory += local_memory;
        total.shared_memory += shared_memory;
        total.clients += 1;
        usage.push(GpuProcessUsage {
            pid,
            device: devices[device].key.clone(),
            utilization: engines.busiest().unwrap_or(Percent::ZERO),
            engines,
            memory: local_memory,
            shared_memory,
        });
    }

    fn finish(&mut self, usage: &mut Vec<GpuProcessUsage>) {
        let pass = self.pass;
        self.states.retain(|_, state| state.pass == pass);
        merge_by_process(usage);
    }
}

fn resolve_device(client: &DrmClient<'_>, devices: &[Arc<GpuIdentity>]) -> Option<usize> {
    if let Some(pdev) = client.pdev {
        return devices
            .iter()
            .position(|device| device.key.as_str().as_bytes() == pdev);
    }
    let driver = client.driver?;
    devices
        .iter()
        .position(|device| device.driver.as_bytes() == driver)
}

fn first_observation(current: &EngineCounters) -> EngineUsage {
    let mut usage = EngineUsage::default();
    for class in EngineClass::ALL {
        if current[class.index()].is_present() {
            usage.set(class, Percent::ZERO);
        }
    }
    usage
}

fn rates(previous: &EngineCounters, current: &EngineCounters, elapsed: Duration) -> EngineUsage {
    let mut usage = EngineUsage::default();
    let wall_nanoseconds = elapsed.as_nanos() as f64;
    for class in EngineClass::ALL {
        let index = class.index();
        if let Some(rate) = engine_rate(&previous[index], &current[index], wall_nanoseconds) {
            usage.set(class, rate);
        }
    }
    usage
}

fn engine_rate(
    previous: &EngineCounter,
    current: &EngineCounter,
    wall_nanoseconds: f64,
) -> Option<Percent> {
    if !current.is_present() {
        return None;
    }
    let busy = current.busy.saturating_sub(previous.busy) as f64;
    let span = if current.counts_cycles() {
        current.total_cycles.saturating_sub(previous.total_cycles) as f64
    } else {
        wall_nanoseconds
    };
    if span <= 0.0 {
        return Some(Percent::ZERO);
    }
    let ratio = busy / span / f64::from(current.capacity());
    Some(Percent::clamped((ratio * PERCENT) as f32))
}

pub(crate) fn merge_by_process(usage: &mut Vec<GpuProcessUsage>) {
    usage.sort_unstable_by(|left, right| (left.pid, &left.device).cmp(&(right.pid, &right.device)));
    usage.dedup_by(|duplicate, kept| {
        if duplicate.pid != kept.pid || duplicate.device != kept.device {
            return false;
        }
        kept.engines.merge(&duplicate.engines);
        kept.memory += duplicate.memory;
        kept.shared_memory += duplicate.shared_memory;
        kept.utilization = kept.engines.busiest().unwrap_or(Percent::ZERO);
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::fdinfo::tests::AMDGPU_FDINFO;
    use monitor_core::DeviceKey;

    fn counters(fdinfo: &str) -> EngineCounters {
        fdinfo::parse(fdinfo.as_bytes()).engines
    }

    #[test]
    fn computes_time_based_rate() {
        let earlier = counters(AMDGPU_FDINFO);
        let later_text = AMDGPU_FDINFO.replace("46849625849 ns", "47349625849 ns");
        let later = counters(&later_text);
        let usage = rates(&earlier, &later, Duration::from_secs(1));
        assert_eq!(usage.get(EngineClass::Graphics), Some(Percent::new(50.0)));
        assert_eq!(usage.get(EngineClass::Compute), Some(Percent::ZERO));
        assert_eq!(usage.get(EngineClass::Copy), None);
    }

    #[test]
    fn clamps_and_survives_counter_reset() {
        let earlier = counters("drm-engine-gfx:\t100 ns\n");
        let later = counters("drm-engine-gfx:\t5000000000 ns\n");
        let usage = rates(&earlier, &later, Duration::from_secs(1));
        assert_eq!(usage.get(EngineClass::Graphics), Some(Percent::FULL));
        let reset = rates(&later, &earlier, Duration::from_secs(1));
        assert_eq!(reset.get(EngineClass::Graphics), Some(Percent::ZERO));
    }

    #[test]
    fn computes_cycle_based_rate_with_capacity() {
        let earlier = counters(
            "drm-cycles-vcs:\t0\ndrm-total-cycles-vcs:\t1000\ndrm-engine-capacity-vcs:\t2\n",
        );
        let later = counters(
            "drm-cycles-vcs:\t1000\ndrm-total-cycles-vcs:\t2000\ndrm-engine-capacity-vcs:\t2\n",
        );
        let usage = rates(&earlier, &later, Duration::ZERO);
        assert_eq!(usage.get(EngineClass::Video), Some(Percent::new(50.0)));
    }

    #[test]
    fn merges_clients_of_one_process() {
        let device = DeviceKey::new("0000:06:00.0");
        let entry = |pid: u32, graphics: f32, memory: u64| {
            let mut engines = EngineUsage::default();
            engines.set(EngineClass::Graphics, Percent::new(graphics));
            GpuProcessUsage {
                pid: Pid::new(pid),
                device: device.clone(),
                utilization: Percent::new(graphics),
                engines,
                memory: Bytes::new(memory),
                shared_memory: Bytes::ZERO,
            }
        };
        let mut usage = vec![entry(7, 10.0, 1), entry(3, 1.0, 1), entry(7, 15.0, 2)];
        merge_by_process(&mut usage);
        assert_eq!(usage.len(), 2);
        assert_eq!(usage[1].pid, Pid::new(7));
        assert_eq!(usage[1].utilization, Percent::new(25.0));
        assert_eq!(usage[1].memory, Bytes::new(3));
    }
}
