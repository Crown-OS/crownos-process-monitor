//! The `/org/crownos/ProcessMonitor1` object. Handlers reach the helper's
//! state only through [`ProcessMonitor::authorized`], which identifies the
//! caller and asks the [`Authorizer`] for the method's one capability.

mod method;
mod state;

use std::sync::Arc;

use zbus::fdo::DBusProxy;
use zbus::message::Header;
use zbus::names::{BusName, OwnedUniqueName};

pub use method::{Method, capability_for};
pub use state::HelperState;

use crate::auth::{AuthError, Authorizer, Decision};
use crate::sources::NETWORK_ACCOUNTING_SUPPORTED;
use crate::util::lock::lock;
use crate::{
    API_VERSION, BUS_NAME, Caller, Capability, CgroupTraffic, DrmClient, EnergyZone, HelperError,
    OBJECT_PATH, ProcessUsage, SocketOwner, actions,
};

/// Exports the service on `connection` and takes the well-known name.
pub async fn serve<A: Authorizer>(
    connection: &zbus::Connection,
    state: Arc<HelperState<A>>,
) -> zbus::Result<()> {
    let service = ProcessMonitor {
        state,
        bus: DBusProxy::new(connection).await?,
    };
    connection.object_server().at(OBJECT_PATH, service).await?;
    connection.request_name(BUS_NAME).await
}

/// The capabilities this build can serve.
pub fn supported_capabilities() -> impl Iterator<Item = Capability> {
    Capability::ALL.into_iter().filter(|&capability| {
        capability != Capability::ReadNetworkAccounting || NETWORK_ACCOUNTING_SUPPORTED
    })
}

pub struct ProcessMonitor<A> {
    state: Arc<HelperState<A>>,
    bus: DBusProxy<'static>,
}

impl<A: Authorizer> ProcessMonitor<A> {
    /// Authorizes `method` for the sender of `header`, then runs `work` on
    /// the blocking pool.
    async fn authorized<T, Work>(
        &self,
        header: &Header<'_>,
        method: Method,
        work: Work,
    ) -> Result<T, HelperError>
    where
        T: Send + 'static,
        Work: FnOnce(&HelperState<A>, &Caller) -> Result<T, HelperError> + Send + 'static,
    {
        let caller = self.admit(header, method).await?;
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || work(&state, &caller))
            .await
            .map_err(HelperError::failed)?
    }

    async fn admit(&self, header: &Header<'_>, method: Method) -> Result<Caller, HelperError> {
        let sender = header
            .sender()
            .map(|sender| OwnedUniqueName::from(sender.to_owned()))
            .ok_or_else(|| HelperError::NotAuthorized("the call has no sender".to_owned()))?;
        self.state.clients.arrived(sender.as_str());
        let caller = match Caller::identify(&self.bus, sender.clone()).await {
            Ok(caller) => caller,
            Err(error) => {
                self.forget_if_gone(&sender).await;
                return Err(HelperError::NotAuthorized(error.to_string()));
            }
        };
        let capability = method.capability();
        let refusal = || format!("{} needs {}", method.name(), capability.name());
        match self.state.authorizer.authorize(&caller, capability).await {
            Ok(Decision::Allowed) => Ok(caller),
            Ok(Decision::Denied) => Err(HelperError::NotAuthorized(refusal())),
            Ok(Decision::NeedsAuthentication) => Err(HelperError::NeedsAuthentication(refusal())),
            Err(AuthError::Credentials(reason)) => {
                Err(HelperError::NotAuthorized(reason.to_owned()))
            }
            Err(error) => Err(HelperError::failed(error)),
        }
    }

    /// A caller that vanished before it could be identified never gets a
    /// `NameOwnerChanged` after its arrival, so it is dropped here.
    async fn forget_if_gone(&self, sender: &OwnedUniqueName) {
        let present = self
            .bus
            .name_has_owner(BusName::Unique(sender.as_ref()))
            .await
            .unwrap_or(false);
        if !present && let Err(error) = self.state.client_departed(sender.as_str()) {
            eprintln!("monitor-helper: restoring fans of {sender} failed: {error}");
        }
    }
}

#[zbus::interface(name = "org.crownos.ProcessMonitor1")]
impl<A: Authorizer> ProcessMonitor<A> {
    /// Disk bytes and PSS/USS/swap of `pids`, or of every process when
    /// empty. Memory figures are measured round-robin and 0 until then.
    async fn process_usage(
        &self,
        #[zbus(header)] header: Header<'_>,
        pids: Vec<u32>,
    ) -> Result<Vec<ProcessUsage>, HelperError> {
        self.authorized(&header, Method::ProcessUsage, move |state, _| {
            state.sources.process_usage(&pids)
        })
        .await
    }

    /// The process holding each socket inode, for every user.
    async fn socket_owners(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<Vec<SocketOwner>, HelperError> {
        self.authorized(&header, Method::SocketOwners, |state, _| {
            state.sources.socket_owners()
        })
        .await
    }

    /// The fdinfo text of every DRM file descriptor of every process.
    async fn drm_clients(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<Vec<DrmClient>, HelperError> {
        self.authorized(&header, Method::DrmClients, |state, _| {
            state.sources.drm_clients()
        })
        .await
    }

    /// Raw RAPL counters in microjoules.
    async fn energy(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<Vec<EnergyZone>, HelperError> {
        self.authorized(&header, Method::Energy, |state, _| {
            Ok(state.sources.energy())
        })
        .await
    }

    /// Bytes per cgroup, every protocol; `NotSupported` without eBPF.
    async fn network_accounting(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<Vec<CgroupTraffic>, HelperError> {
        self.authorized(&header, Method::NetworkAccounting, |state, _| {
            state.sources.network_accounting()
        })
        .await
    }

    /// Fixes a fan at `percent` duty until the calling client restores it
    /// or leaves the bus.
    async fn set_fan(
        &self,
        #[zbus(header)] header: Header<'_>,
        fan_id: String,
        percent: u8,
    ) -> Result<(), HelperError> {
        self.authorized(&header, Method::SetFan, move |state, caller| {
            lock(&state.fans).set(&fan_id, percent, caller.bus_name.as_str())
        })
        .await
    }

    /// Hands every manually set fan back to automatic control.
    async fn restore_fans(&self, #[zbus(header)] header: Header<'_>) -> Result<(), HelperError> {
        self.authorized(&header, Method::RestoreFans, |state, _| {
            lock(&state.fans).restore_all()
        })
        .await
    }

    /// Sends `term`, `kill`, `stop` or `cont` to the process started at
    /// `start_ticks`.
    async fn signal(
        &self,
        #[zbus(header)] header: Header<'_>,
        pid: u32,
        start_ticks: u64,
        signal: String,
    ) -> Result<(), HelperError> {
        self.authorized(&header, Method::Signal, move |_, _| {
            actions::signal(pid, start_ticks, &signal)
        })
        .await
    }

    async fn renice(
        &self,
        #[zbus(header)] header: Header<'_>,
        pid: u32,
        start_ticks: u64,
        nice: i32,
    ) -> Result<(), HelperError> {
        self.authorized(&header, Method::Renice, move |_, _| {
            actions::renice(pid, start_ticks, nice)
        })
        .await
    }

    #[zbus(property(emits_changed_signal = "const"))]
    fn version(&self) -> u32 {
        API_VERSION
    }

    /// Capability names this build supports.
    #[zbus(property(emits_changed_signal = "const"))]
    fn capabilities(&self) -> Vec<String> {
        supported_capabilities()
            .map(|capability| capability.name().to_owned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_network_accounting_only_with_ebpf() {
        let supported: Vec<_> = supported_capabilities().collect();
        assert_eq!(
            supported.contains(&Capability::ReadNetworkAccounting),
            cfg!(feature = "ebpf")
        );
        assert!(supported.contains(&Capability::ReadAllProcesses));
    }
}
