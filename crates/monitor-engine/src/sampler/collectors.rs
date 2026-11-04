use std::sync::Arc;
use std::time::{Duration, Instant};

use monitor_core::util::procfs::ProcRoot;
use monitor_core::{
    Collector, Elapsed, FdIndex, FdScope, Interest, Pid, ProcessKey, SampleContext, SourceError,
    SourceResult, Tick,
};
use monitor_hardware::{EnergyCounter, GpuCollector, PowerCollector, SensorCollector};
use monitor_helper::{DrmClient, EnergyZone, SocketOwner};
use monitor_io::{DiskCollector, NetworkCollector, UDisks, wants_socket_owners};
use monitor_system::{
    CpuCollector, MemoryCollector, PrivilegedUsage, ProcessCollector, ProcessDetails,
};

use crate::{Availability, Domain, Snapshot};

/// Helper socket owners younger than this still label the Network page's
/// connections on ticks without a new answer.
const HELPER_SOCKETS_SHOWN_FOR: Duration = Duration::from_secs(3);

/// The helper's latest socket owners and whether a traffic pass used them.
struct HelperSockets {
    owners: Arc<Vec<SocketOwner>>,
    received_at: Duration,
    used: bool,
}

/// Every collector, opened once. A collector that could not be opened stays
/// `None` and its domain is reported unavailable on every tick.
pub(crate) struct Collectors {
    root: Option<ProcRoot>,
    fds: FdIndex,
    /// Descriptor scopes the privileged helper reads for every user, so the
    /// local walk skips them.
    helper_scopes: FdScope,
    helper_sockets: Option<HelperSockets>,
    processes: Option<ProcessCollector>,
    cpu: Option<CpuCollector>,
    memory: Option<MemoryCollector>,
    disk: Option<DiskCollector>,
    network: NetworkCollector,
    gpu: GpuCollector,
    sensors: SensorCollector,
    power: PowerCollector,
}

impl Collectors {
    pub fn open(availability: &mut Availability) -> Self {
        Self {
            root: ProcRoot::open().ok(),
            fds: FdIndex::default(),
            helper_scopes: FdScope::default(),
            helper_sockets: None,
            processes: opened(availability, Domain::Processes, ProcessCollector::new()),
            cpu: opened(availability, Domain::Cpu, CpuCollector::new()),
            memory: opened(availability, Domain::Memory, MemoryCollector::new()),
            disk: opened(availability, Domain::Disk, DiskCollector::new()),
            network: NetworkCollector::new(),
            gpu: GpuCollector::new(),
            sensors: SensorCollector::new(),
            power: PowerCollector::new(),
        }
    }

    pub fn udisks(&self) -> Option<UDisks> {
        self.disk.as_ref().map(DiskCollector::udisks)
    }

    pub fn inspect(&self, key: ProcessKey) -> SourceResult<ProcessDetails> {
        self.processes
            .as_ref()
            .ok_or(SourceError::Unavailable("the process table"))?
            .inspect(key)
    }

    /// One pass over every domain, in the order their inputs depend on:
    /// the process sweep first, so the fd index covers this tick's pids.
    pub fn sample(
        &mut self,
        tick: Tick,
        elapsed: Elapsed,
        interest: &Interest,
        out: &mut Snapshot,
    ) {
        let mut lap = Lap::start();
        let processes = run(
            self.processes.as_mut(),
            &SampleContext {
                tick,
                elapsed,
                interest,
                fds: &self.fds,
            },
            &mut out.processes,
        );
        out.availability.record(Domain::Processes, processes);
        out.costs.set(Domain::Processes, lap.next());
        self.refresh_fds(interest, &tick);
        out.costs.fd_index = lap.next();
        let context = SampleContext {
            tick,
            elapsed,
            interest,
            fds: &self.fds,
        };
        let status = &mut out.availability;
        let costs = &mut out.costs;
        status.record(Domain::Cpu, run(self.cpu.as_mut(), &context, &mut out.cpu));
        costs.set(Domain::Cpu, lap.next());
        status.record(
            Domain::Memory,
            run(self.memory.as_mut(), &context, &mut out.memory),
        );
        costs.set(Domain::Memory, lap.next());
        status.record(
            Domain::Disk,
            run(self.disk.as_mut(), &context, &mut out.disk),
        );
        costs.set(Domain::Disk, lap.next());
        status.record(
            Domain::Network,
            self.network.sample(&context, &mut out.network),
        );
        costs.set(Domain::Network, lap.next());
        status.record(Domain::Gpu, self.gpu.sample(&context, &mut out.gpu));
        costs.set(Domain::Gpu, lap.next());
        status.record(
            Domain::Sensors,
            self.sensors.sample(&context, &mut out.sensors),
        );
        costs.set(Domain::Sensors, lap.next());
        status.record(Domain::Power, self.power.sample(&context, &mut out.power));
        costs.set(Domain::Power, lap.next());
    }

    /// Walks `/proc/<pid>/fd` of the user's processes only when a socket or
    /// DRM view (or history, every few seconds) needs it, and only of
    /// processes that changed; scopes the helper covers come from its
    /// answers instead.
    fn refresh_fds(&mut self, interest: &Interest, tick: &Tick) {
        let wanted = aligned_while_minimized(
            FdScope {
                sockets: self.sockets_due(interest, tick),
                drm: self.gpu.drm_fds_due(interest, tick),
            },
            interest,
        );
        let local = FdScope {
            sockets: wanted.sockets && !self.helper_scopes.sockets,
            drm: wanted.drm && !self.helper_scopes.drm,
        };
        if !(local.is_empty() && self.fds.scope().is_empty())
            && let (Some(root), Some(processes)) = (&self.root, &self.processes)
        {
            self.fds.refresh_changed(root, processes.own_pids(), local);
        }
        if self.helper_scopes.sockets {
            self.install_helper_sockets(interest, tick.monotonic());
        }
    }

    /// A new helper answer makes this tick a traffic pass; an older one
    /// still names connection owners while the Network page is open.
    fn install_helper_sockets(&mut self, interest: &Interest, now: Duration) {
        let Some(sockets) = self.helper_sockets.as_mut() else {
            return;
        };
        let shown = wants_socket_owners(interest)
            && now.saturating_sub(sockets.received_at) < HELPER_SOCKETS_SHOWN_FOR;
        if !sockets.used || shown {
            sockets.used = true;
            self.fds.replace_sockets(
                sockets
                    .owners
                    .iter()
                    .map(|owner| (owner.inode, Pid::new(owner.pid))),
            );
        }
    }

    pub fn sockets_due(&self, interest: &Interest, tick: &Tick) -> bool {
        self.network.socket_owners_due(interest, tick)
    }

    pub fn drm_due(&self, interest: &Interest, tick: &Tick) -> bool {
        self.gpu.drm_fds_due(interest, tick)
    }

    /// The descriptor scopes the privileged helper covers for every user.
    pub fn set_helper_scopes(&mut self, scopes: FdScope) {
        self.helper_scopes = scopes;
        if !scopes.sockets {
            self.helper_sockets = None;
        }
    }

    pub fn apply_privileged_usage(&mut self, usage: &[PrivilegedUsage], now: Duration) {
        if let Some(processes) = self.processes.as_mut() {
            processes.apply_privileged(usage, now);
        }
    }

    pub fn offer_helper_sockets(&mut self, owners: Arc<Vec<SocketOwner>>, now: Duration) {
        self.helper_sockets = Some(HelperSockets {
            owners,
            received_at: now,
            used: false,
        });
    }

    pub fn use_helper_drm_clients(&mut self, clients: &[DrmClient], now: Duration) {
        self.gpu.use_external_clients(
            clients
                .iter()
                .map(|client| (Pid::new(client.pid), client.fdinfo.as_bytes())),
            now,
        );
    }

    pub fn use_helper_energy(&mut self, zones: &[EnergyZone], now: Duration) {
        self.power.use_external_energy(
            zones.iter().map(|zone| EnergyCounter {
                zone: &zone.zone,
                microjoules: zone.energy_microjoules,
                max_range_microjoules: zone.max_range_microjoules,
            }),
            now,
        );
    }
}

/// One walk records both scopes, so while minimized a pass due for either
/// takes both: socket owners and DRM clients then share one 15 s history pass
/// instead of walking on separate ticks.
fn aligned_while_minimized(wanted: FdScope, interest: &Interest) -> FdScope {
    if interest.is_visible() || wanted.is_empty() {
        wanted
    } else {
        FdScope {
            sockets: true,
            drm: true,
        }
    }
}

/// Wall time between consecutive calls.
struct Lap(Instant);

impl Lap {
    fn start() -> Self {
        Self(Instant::now())
    }

    fn next(&mut self) -> Duration {
        let now = Instant::now();
        let lap = now.duration_since(self.0);
        self.0 = now;
        lap
    }
}

fn opened<C>(
    availability: &mut Availability,
    domain: Domain,
    result: SourceResult<C>,
) -> Option<C> {
    result
        .inspect_err(|error| availability.mark_unavailable(domain, error))
        .ok()
}

fn run<C: Collector>(
    collector: Option<&mut C>,
    context: &SampleContext<'_>,
    output: &mut C::Output,
) -> SourceResult<()> {
    match collector {
        Some(collector) => collector.sample(context, output),
        None => Err(SourceError::Unavailable("this source")),
    }
}

#[cfg(test)]
mod tests {
    use monitor_core::WindowState;

    use super::*;

    fn minimized() -> Interest {
        Interest {
            window: WindowState::Minimized,
            ..Interest::default()
        }
    }

    #[test]
    fn minimized_passes_take_both_scopes_together() {
        let drm_only = FdScope {
            sockets: false,
            drm: true,
        };
        let both = FdScope {
            sockets: true,
            drm: true,
        };
        assert_eq!(aligned_while_minimized(drm_only, &minimized()), both);
        assert_eq!(
            aligned_while_minimized(drm_only, &Interest::default()),
            drm_only
        );
        assert!(aligned_while_minimized(FdScope::default(), &minimized()).is_empty());
    }

    /// Back-to-back minimized ticks: the first is the history pass (fd walk
    /// and device read), the next within 15 s walks nothing and carries the
    /// GPU figures; a visible tick reads the devices again.
    #[test]
    fn minimized_ticks_between_history_passes_skip_the_walk_and_carry_gpus() {
        let mut collectors = Collectors::open(&mut Availability::default());
        let mut snapshot = Snapshot::default();
        let tick = |collectors: &mut Collectors, snapshot: &mut Snapshot, interest| {
            collectors.sample(Tick::now(), Elapsed::default(), &interest, snapshot);
        };
        tick(&mut collectors, &mut snapshot, minimized());
        let pass = collectors.fds.scope();
        let walked = collectors.root.is_some() && collectors.processes.is_some();
        assert!(!walked || pass.drm, "the first minimized tick is a pass");
        assert!(!walked || pass.sockets || collectors.helper_scopes.sockets);
        let read = snapshot.gpu.devices.clone();

        tick(&mut collectors, &mut snapshot, minimized());
        assert!(collectors.fds.scope().is_empty(), "no walk between passes");
        let carried: Vec<_> = snapshot
            .gpu
            .devices
            .iter()
            .map(|device| device.metrics)
            .collect();
        let first: Vec<_> = read.iter().map(|device| device.metrics).collect();
        assert_eq!(
            carried, first,
            "GPU figures are carried until the next pass"
        );
    }
}
