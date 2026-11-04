//! The optional privileged helper (plan §4.9, decision D2). Every read is
//! requested on one tick and its answer applied on a later one, so the
//! sampler never waits on the bus; without the helper nothing changes.

mod accounting;
mod actions;

use std::sync::Arc;
use std::time::Duration;

use monitor_core::{FdScope, Interest, Page, Pid, ProcessKey, Tick, Uid};
use monitor_helper::{Capability, HelperAvailability, HelperClient, ProcessUsage};
use monitor_system::{IoCounters, PrivilegedUsage, SmapsRollup};

pub use actions::{HelperAction, HelperFailure, HelperOutcome};
pub(crate) use actions::{OUTCOME_BACKLOG, perform};

use self::accounting::CgroupAccounting;
use crate::Snapshot;
use crate::sampler::Collectors;

/// While the helper refuses reads, they are retried this rarely.
const DENIED_RETRY: Duration = Duration::from_secs(30);

/// How the engine reaches the helper.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum HelperConnection {
    /// The system bus service, when its D-Bus activation file is installed.
    #[default]
    System,
    /// A bus address, for development and tests.
    Address(String),
    Disabled,
}

/// What the optional privileged helper contributes.
#[derive(Clone, Debug, PartialEq)]
pub struct HelperStatus {
    /// `Available` once some read was answered, `Denied` when reads were
    /// only refused, `Absent` without a helper.
    pub availability: HelperAvailability,
    /// What the installed helper build serves, minus reads it refused;
    /// empty until it answered.
    pub capabilities: Arc<[Capability]>,
}

impl Default for HelperStatus {
    fn default() -> Self {
        Self {
            availability: HelperAvailability::Absent,
            capabilities: Arc::from([]),
        }
    }
}

impl HelperStatus {
    pub fn serves(&self, capability: Capability) -> bool {
        self.availability == HelperAvailability::Available
            && self.capabilities.contains(&capability)
    }
}

/// The last answer of one read kind that was applied; a new `Arc` in the
/// client's slot is a new answer.
struct Seen<T>(Option<Arc<T>>);

impl<T> Seen<T> {
    fn new(initial: Option<Arc<T>>) -> Self {
        Self(initial)
    }

    fn fresh(&mut self, latest: Option<Arc<T>>) -> Option<Arc<T>> {
        let latest = latest?;
        if self
            .0
            .as_ref()
            .is_some_and(|seen| Arc::ptr_eq(seen, &latest))
        {
            return None;
        }
        self.0 = Some(Arc::clone(&latest));
        Some(latest)
    }
}

pub(crate) struct Privileged {
    client: HelperClient,
    own_uid: Uid,
    process_usage: Seen<Vec<ProcessUsage>>,
    socket_owners: Seen<Vec<monitor_helper::SocketOwner>>,
    drm_clients: Seen<Vec<monitor_helper::DrmClient>>,
    energy: Seen<Vec<monitor_helper::EnergyZone>>,
    network: Seen<Vec<monitor_helper::CgroupTraffic>>,
    accounting: CgroupAccounting,
    usage: Vec<PrivilegedUsage>,
    denied_retries: [Option<Duration>; Capability::ALL.len()],
}

impl Privileged {
    /// `None` when the helper is disabled or not installed.
    pub fn connect(connection: &HelperConnection) -> Option<Self> {
        let client = match connection {
            HelperConnection::System => HelperClient::connect()?,
            HelperConnection::Address(address) => {
                HelperClient::connect_to_address(address.as_str())?
            }
            HelperConnection::Disabled => return None,
        };
        Some(Self {
            process_usage: Seen::new(client.latest_process_usage()),
            socket_owners: Seen::new(client.latest_socket_owners()),
            drm_clients: Seen::new(client.latest_drm_clients()),
            energy: Seen::new(client.latest_energy()),
            network: Seen::new(client.latest_network_accounting()),
            client,
            own_uid: Uid::current(),
            accounting: CgroupAccounting::default(),
            usage: Vec::new(),
            denied_retries: [None; Capability::ALL.len()],
        })
    }

    pub fn client(&self) -> &HelperClient {
        &self.client
    }

    /// The helper answered reads needing `capability`.
    fn serves(&self, capability: Capability) -> bool {
        self.client.supports(capability)
            && self.client.capability_availability(capability) == HelperAvailability::Available
    }

    /// Supported and not refused, or refused long enough ago to ask again.
    fn may_request(&mut self, capability: Capability, now: Duration) -> bool {
        if !self.client.supports(capability) {
            return false;
        }
        if self.client.capability_availability(capability) != HelperAvailability::Denied {
            return true;
        }
        let retry = &mut self.denied_retries[capability as usize];
        let due = retry.is_none_or(|last| now.saturating_sub(last) >= DENIED_RETRY);
        if due {
            *retry = Some(now);
        }
        due
    }

    /// Applies the answers that arrived since the previous tick, and tells
    /// the collectors which descriptor scopes the helper now covers.
    pub fn consume(&mut self, collectors: &mut Collectors, now: Duration) {
        collectors.set_helper_scopes(FdScope {
            sockets: self.serves(Capability::ReadAllProcesses),
            drm: self.serves(Capability::ReadGpuCounters),
        });
        if let Some(answer) = self.process_usage.fresh(self.client.latest_process_usage()) {
            self.usage.clear();
            self.usage.extend(answer.iter().map(privileged_usage));
            collectors.apply_privileged_usage(&self.usage, now);
        }
        if let Some(answer) = self.socket_owners.fresh(self.client.latest_socket_owners()) {
            collectors.offer_helper_sockets(answer, now);
        }
        if let Some(answer) = self.drm_clients.fresh(self.client.latest_drm_clients()) {
            collectors.use_helper_drm_clients(&answer, now);
        }
        if let Some(answer) = self.energy.fresh(self.client.latest_energy()) {
            collectors.use_helper_energy(&answer, now);
        }
        if let Some(answer) = self.network.fresh(self.client.latest_network_accounting()) {
            self.accounting.update(answer, now);
        }
    }

    /// Per-app all-protocol traffic, after the TCP join.
    pub fn apply_accounting(&self, snapshot: &mut Snapshot) {
        if self.accounting.is_measuring() && self.serves(Capability::ReadNetworkAccounting) {
            self.accounting.apply(&mut snapshot.processes.apps);
            snapshot.network.process_traffic_coverage = monitor_core::Coverage::Full;
        }
    }

    /// Asks for what the next tick will need, before this tick's sample
    /// satisfies it locally; process ids come from the previous tick.
    pub fn request(
        &mut self,
        interest: &Interest,
        collectors: &Collectors,
        previous: &Snapshot,
        tick: &Tick,
    ) {
        let now = tick.monotonic();
        if self.may_request(Capability::ReadAllProcesses, now) {
            if wants_process_usage(interest) {
                self.client
                    .request_process_usage(self.other_users(previous));
            }
            if collectors.sockets_due(interest, tick) {
                self.client.request_socket_owners();
            }
        }
        if self.may_request(Capability::ReadGpuCounters, now) && collectors.drm_due(interest, tick)
        {
            self.client.request_drm_clients();
        }
        if self.may_request(Capability::ReadEnergy, now) {
            self.client.request_energy();
        }
        if self.may_request(Capability::ReadNetworkAccounting, now) {
            self.client.request_network_accounting();
        }
    }

    /// Processes this user cannot read I/O or PSS of.
    fn other_users(&self, snapshot: &Snapshot) -> Vec<u32> {
        snapshot
            .processes
            .processes
            .iter()
            .filter(|row| row.uid != self.own_uid && !row.kernel_thread)
            .map(|row| row.key.pid.get())
            .collect()
    }

    /// Available once any read was answered, Denied when reads were only
    /// refused; the capabilities exclude refused reads.
    pub fn publish_status(&self, snapshot: &mut Snapshot) {
        let status = &mut snapshot.helper;
        let read_states = Capability::ALL
            .into_iter()
            .filter(|capability| capability.is_read_only())
            .map(|capability| self.client.capability_availability(capability));
        status.availability = if read_states
            .clone()
            .any(|state| state == HelperAvailability::Available)
        {
            HelperAvailability::Available
        } else if read_states
            .clone()
            .any(|state| state == HelperAvailability::Denied)
        {
            HelperAvailability::Denied
        } else {
            HelperAvailability::Absent
        };
        let served = Capability::ALL.into_iter().filter(|&capability| {
            self.client.supports(capability)
                && self.client.capability_availability(capability) != HelperAvailability::Denied
        });
        if !served.clone().eq(status.capabilities.iter().copied()) {
            status.capabilities = served.collect();
        }
    }
}

/// Process I/O and PSS are shown on these views only.
fn wants_process_usage(interest: &Interest) -> bool {
    interest.is_visible()
        && (interest.inspected.is_some()
            || matches!(interest.page, Page::Overview | Page::Memory | Page::Disk))
}

fn privileged_usage(usage: &ProcessUsage) -> PrivilegedUsage {
    let measured = usage.pss > 0 || usage.uss > 0;
    PrivilegedUsage {
        key: ProcessKey {
            pid: Pid::new(usage.pid),
            start_ticks: usage.start_ticks,
        },
        io: IoCounters {
            read_bytes: usage.read_bytes,
            write_bytes: usage.written_bytes,
            cancelled_write_bytes: usage.cancelled_written_bytes,
            ..IoCounters::default()
        },
        rollup: measured.then(|| SmapsRollup {
            proportional: monitor_core::Bytes::new(usage.pss),
            unique: monitor_core::Bytes::new(usage.uss),
            swap: monitor_core::Bytes::new(usage.swap),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_answers_are_seen_once() {
        let first = Arc::new(vec![1]);
        let mut seen = Seen::new(Some(Arc::clone(&first)));
        assert!(seen.fresh(Some(Arc::clone(&first))).is_none());
        let second = Arc::new(vec![2]);
        assert!(seen.fresh(Some(Arc::clone(&second))).is_some());
        assert!(seen.fresh(Some(second)).is_none());
        assert!(seen.fresh(None).is_none());
    }

    #[test]
    fn unmeasured_pss_stays_unknown() {
        let usage = ProcessUsage {
            pid: 7,
            start_ticks: 9,
            read_bytes: 4096,
            ..ProcessUsage::default()
        };
        let converted = privileged_usage(&usage);
        assert_eq!(converted.io.read_bytes, 4096);
        assert!(converted.rollup.is_none());
        assert!(
            privileged_usage(&ProcessUsage { pss: 1, ..usage })
                .rollup
                .is_some()
        );
    }

    #[test]
    fn disabled_helper_does_not_connect() {
        assert!(Privileged::connect(&HelperConnection::Disabled).is_none());
    }
}
