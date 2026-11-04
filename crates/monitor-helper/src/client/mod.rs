//! The engine's side of the helper. Calls run on a dedicated thread; read
//! results land in slots read with `try_lock`, so the sampler never waits
//! on the bus.

mod proxy;
mod worker;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use monitor_core::ProcessKey;
use tokio::sync::{mpsc, oneshot};
use worker::{BusTarget, answer};

use crate::{
    BUS_NAME, Capability, CgroupTraffic, DrmClient, EnergyZone, HelperError, ProcessUsage,
    SignalKind, SocketOwner,
};
use monitor_core::util::latest::Latest;

const SYSTEM_SERVICE_DIRECTORIES: [&str; 3] = [
    "/usr/local/share/dbus-1/system-services",
    "/usr/share/dbus-1/system-services",
    "/lib/dbus-1/system-services",
];

/// Whether pages can show figures only the helper measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HelperAvailability {
    /// Not installed, not reachable, or no read answered yet.
    Absent,
    /// Reachable, but the authorization backend refused the reads.
    Denied,
    Available,
}

impl HelperAvailability {
    const fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Denied,
            2 => Self::Available,
            _ => Self::Absent,
        }
    }

    fn after<T>(result: &Result<T, HelperError>) -> Option<Self> {
        match result {
            Ok(_) => Some(Self::Available),
            Err(error) if error.is_denial() => Some(Self::Denied),
            Err(HelperError::ZBus(_)) => Some(Self::Absent),
            Err(_) => None,
        }
    }
}

/// The outcome of a write request, delivered once the helper answered
/// (after any authentication dialog).
pub type ActionReceiver = oneshot::Receiver<Result<(), HelperError>>;
type Reply = oneshot::Sender<Result<(), HelperError>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadKind {
    ProcessUsage,
    SocketOwners,
    DrmClients,
    Energy,
    NetworkAccounting,
}

impl ReadKind {
    const COUNT: usize = 5;

    const fn capability(self) -> Capability {
        match self {
            Self::ProcessUsage | Self::SocketOwners => Capability::ReadAllProcesses,
            Self::DrmClients => Capability::ReadGpuCounters,
            Self::Energy => Capability::ReadEnergy,
            Self::NetworkAccounting => Capability::ReadNetworkAccounting,
        }
    }
}

enum Command {
    ProcessUsage(Vec<u32>),
    SocketOwners,
    DrmClients,
    Energy,
    NetworkAccounting,
    SetFan {
        fan_id: String,
        percent: u8,
        reply: Reply,
    },
    RestoreFans {
        reply: Reply,
    },
    Signal {
        key: ProcessKey,
        signal: SignalKind,
        reply: Reply,
    },
    Renice {
        key: ProcessKey,
        nice: i32,
        reply: Reply,
    },
}

impl Command {
    fn into_reply(self) -> Option<Reply> {
        match self {
            Self::SetFan { reply, .. }
            | Self::RestoreFans { reply }
            | Self::Signal { reply, .. }
            | Self::Renice { reply, .. } => Some(reply),
            Self::ProcessUsage(_)
            | Self::SocketOwners
            | Self::DrmClients
            | Self::Energy
            | Self::NetworkAccounting => None,
        }
    }
}

#[derive(Default)]
struct Shared {
    availability: AtomicU8,
    capability_availability: [AtomicU8; Capability::ALL.len()],
    in_flight: [AtomicBool; ReadKind::COUNT],
    capabilities: Latest<Vec<Capability>>,
    process_usage: Latest<Vec<ProcessUsage>>,
    socket_owners: Latest<Vec<SocketOwner>>,
    drm_clients: Latest<Vec<DrmClient>>,
    energy: Latest<Vec<EnergyZone>>,
    network_accounting: Latest<Vec<CgroupTraffic>>,
}

impl Shared {
    fn finish<T>(&self, kind: ReadKind, result: Result<T, HelperError>, slot: &Latest<T>) {
        if let Some(availability) = HelperAvailability::after(&result) {
            self.availability
                .store(availability as u8, Ordering::Relaxed);
            self.capability_availability[kind.capability() as usize]
                .store(availability as u8, Ordering::Relaxed);
        }
        if let Ok(value) = result {
            slot.publish(value);
        }
        self.in_flight[kind as usize].store(false, Ordering::Release);
    }
}

/// A connection to `org.crownos.ProcessMonitor1`. Dropping it disconnects,
/// which makes the helper restore any fans this client set.
pub struct HelperClient {
    commands: mpsc::UnboundedSender<Command>,
    shared: Arc<Shared>,
}

impl HelperClient {
    /// Connects to the system bus helper, or `None` when no D-Bus
    /// activation file for it is installed. Never blocks.
    pub fn connect() -> Option<Self> {
        let installed = SYSTEM_SERVICE_DIRECTORIES.iter().any(|directory| {
            Path::new(directory)
                .join(format!("{BUS_NAME}.service"))
                .exists()
        });
        if installed {
            Self::spawn(BusTarget::System)
        } else {
            None
        }
    }

    /// Connects to a helper on the bus at `address`, for development and
    /// tests.
    pub fn connect_to_address(address: impl Into<String>) -> Option<Self> {
        Self::spawn(BusTarget::Address(address.into()))
    }

    fn spawn(target: BusTarget) -> Option<Self> {
        let (commands, receiver) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared::default());
        worker::spawn(target, receiver, Arc::clone(&shared)).then_some(Self { commands, shared })
    }

    /// Judged from the most recent read request.
    pub fn availability(&self) -> HelperAvailability {
        HelperAvailability::from_raw(self.shared.availability.load(Ordering::Relaxed))
    }

    /// Judged from the most recent read needing `capability`, so a denied
    /// energy read does not hide allowed process reads. `Absent` before the
    /// first such read answered, and for write capabilities.
    pub fn capability_availability(&self, capability: Capability) -> HelperAvailability {
        HelperAvailability::from_raw(
            self.shared.capability_availability[capability as usize].load(Ordering::Relaxed),
        )
    }

    /// Whether the helper build serves `capability` (e.g. network
    /// accounting needs eBPF). `false` until the helper answered once.
    pub fn supports(&self, capability: Capability) -> bool {
        self.shared
            .capabilities
            .try_get()
            .is_some_and(|supported| supported.contains(&capability))
    }

    /// Queues a read unless the previous one of its kind is still running.
    fn request(&self, kind: ReadKind, command: Command) {
        let in_flight = &self.shared.in_flight[kind as usize];
        if in_flight.swap(true, Ordering::AcqRel) {
            return;
        }
        if self.commands.send(command).is_err() {
            in_flight.store(false, Ordering::Release);
        }
    }

    fn act(&self, command: impl FnOnce(Reply) -> Command) -> ActionReceiver {
        let (reply, receiver) = oneshot::channel();
        if let Err(mpsc::error::SendError(command)) = self.commands.send(command(reply))
            && let Some(reply) = command.into_reply()
        {
            answer(
                reply,
                Err(HelperError::failed("the helper client has stopped")),
            );
        }
        receiver
    }

    /// Asks for [`ProcessUsage`] of `pids`, or of every process when empty.
    pub fn request_process_usage(&self, pids: Vec<u32>) {
        self.request(ReadKind::ProcessUsage, Command::ProcessUsage(pids));
    }

    pub fn latest_process_usage(&self) -> Option<Arc<Vec<ProcessUsage>>> {
        self.shared.process_usage.try_get()
    }

    pub fn request_socket_owners(&self) {
        self.request(ReadKind::SocketOwners, Command::SocketOwners);
    }

    pub fn latest_socket_owners(&self) -> Option<Arc<Vec<SocketOwner>>> {
        self.shared.socket_owners.try_get()
    }

    pub fn request_drm_clients(&self) {
        self.request(ReadKind::DrmClients, Command::DrmClients);
    }

    pub fn latest_drm_clients(&self) -> Option<Arc<Vec<DrmClient>>> {
        self.shared.drm_clients.try_get()
    }

    pub fn request_energy(&self) {
        self.request(ReadKind::Energy, Command::Energy);
    }

    pub fn latest_energy(&self) -> Option<Arc<Vec<EnergyZone>>> {
        self.shared.energy.try_get()
    }

    pub fn request_network_accounting(&self) {
        self.request(ReadKind::NetworkAccounting, Command::NetworkAccounting);
    }

    pub fn latest_network_accounting(&self) -> Option<Arc<Vec<CgroupTraffic>>> {
        self.shared.network_accounting.try_get()
    }

    /// `fan_id` is [`crate::fan_id_string`] of the fan.
    pub fn set_fan(&self, fan_id: impl Into<String>, percent: u8) -> ActionReceiver {
        let fan_id = fan_id.into();
        self.act(|reply| Command::SetFan {
            fan_id,
            percent,
            reply,
        })
    }

    pub fn restore_fans(&self) -> ActionReceiver {
        self.act(|reply| Command::RestoreFans { reply })
    }

    pub fn signal(&self, key: ProcessKey, signal: SignalKind) -> ActionReceiver {
        self.act(|reply| Command::Signal { key, signal, reply })
    }

    pub fn renice(&self, key: ProcessKey, nice: i32) -> ActionReceiver {
        self.act(|reply| Command::Renice { key, nice, reply })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_follows_read_results() {
        let denied: Result<(), _> = Err(HelperError::NotAuthorized(String::new()));
        let unsupported: Result<(), _> = Err(HelperError::NotSupported(String::new()));
        let absent: Result<(), _> = Err(HelperError::ZBus(zbus::Error::InterfaceNotFound));
        assert_eq!(
            HelperAvailability::after(&Ok::<_, HelperError>(())),
            Some(HelperAvailability::Available)
        );
        assert_eq!(
            HelperAvailability::after(&denied),
            Some(HelperAvailability::Denied)
        );
        assert_eq!(
            HelperAvailability::after(&absent),
            Some(HelperAvailability::Absent)
        );
        assert_eq!(HelperAvailability::after(&unsupported), None);
        for availability in [
            HelperAvailability::Absent,
            HelperAvailability::Denied,
            HelperAvailability::Available,
        ] {
            assert_eq!(
                HelperAvailability::from_raw(availability as u8),
                availability
            );
        }
    }

    #[test]
    fn unreachable_bus_fails_requests_without_blocking() {
        let Some(client) = HelperClient::connect_to_address("unix:path=/nonexistent/bus") else {
            return;
        };
        assert_eq!(client.availability(), HelperAvailability::Absent);
        client.request_energy();
        let mut outcome = client.restore_fans();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let result = loop {
            match outcome.try_recv() {
                Err(oneshot::error::TryRecvError::Empty)
                    if std::time::Instant::now() < deadline =>
                {
                    std::thread::yield_now();
                }
                other => break other,
            }
        };
        assert!(!matches!(result, Ok(Ok(()))));
        assert!(!client.supports(Capability::ReadEnergy));
    }
}
