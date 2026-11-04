use std::sync::Arc;

use monitor_core::util::runtime_thread::spawn_runtime_thread;
use tokio::sync::mpsc::UnboundedReceiver;
use zbus::proxy::CacheProperties;

use super::proxy::HelperProxy;
use super::{Command, ReadKind, Reply, Shared};
use crate::{Capability, HelperError};

const THREAD_NAME: &str = "monitor-helper-client";

pub(crate) enum BusTarget {
    System,
    Address(String),
}

/// Runs the client on its own thread and current-thread runtime. The
/// thread ends when the [`super::HelperClient`] is dropped.
pub(crate) fn spawn(
    target: BusTarget,
    commands: UnboundedReceiver<Command>,
    shared: Arc<Shared>,
) -> bool {
    spawn_runtime_thread(THREAD_NAME, move || run(target, commands, shared)).is_ok()
}

async fn connect(target: BusTarget) -> zbus::Result<HelperProxy<'static>> {
    let connection = match target {
        BusTarget::System => zbus::Connection::system().await?,
        BusTarget::Address(address) => {
            zbus::connection::Builder::address(address.as_str())
                .build()
                .await?
        }
    };
    HelperProxy::builder(&connection)
        .cache_properties(CacheProperties::No)
        .build()
        .await
}

async fn run(target: BusTarget, mut commands: UnboundedReceiver<Command>, shared: Arc<Shared>) {
    let Ok(proxy) = connect(target).await else {
        return;
    };
    if let Ok(names) = proxy.capabilities().await {
        shared.capabilities.publish(
            names
                .iter()
                .filter_map(|name| Capability::from_name(name))
                .collect(),
        );
    }
    while let Some(command) = commands.recv().await {
        let proxy = proxy.clone();
        let shared = Arc::clone(&shared);
        tokio::spawn(async move { execute(&proxy, command, &shared).await });
    }
}

async fn execute(proxy: &HelperProxy<'_>, command: Command, shared: &Shared) {
    match command {
        Command::ProcessUsage(pids) => {
            let result = proxy.process_usage(&pids).await;
            shared.finish(ReadKind::ProcessUsage, result, &shared.process_usage);
        }
        Command::SocketOwners => {
            let result = proxy.socket_owners().await;
            shared.finish(ReadKind::SocketOwners, result, &shared.socket_owners);
        }
        Command::DrmClients => {
            let result = proxy.drm_clients().await;
            shared.finish(ReadKind::DrmClients, result, &shared.drm_clients);
        }
        Command::Energy => {
            let result = proxy.energy().await;
            shared.finish(ReadKind::Energy, result, &shared.energy);
        }
        Command::NetworkAccounting => {
            let result = proxy.network_accounting().await;
            shared.finish(
                ReadKind::NetworkAccounting,
                result,
                &shared.network_accounting,
            );
        }
        Command::SetFan {
            fan_id,
            percent,
            reply,
        } => answer(reply, proxy.set_fan(&fan_id, percent).await),
        Command::RestoreFans { reply } => answer(reply, proxy.restore_fans().await),
        Command::Signal { key, signal, reply } => answer(
            reply,
            proxy
                .signal(key.pid.get(), key.start_ticks, signal.name())
                .await,
        ),
        Command::Renice { key, nice, reply } => answer(
            reply,
            proxy.renice(key.pid.get(), key.start_ticks, nice).await,
        ),
    }
}

/// A dropped receiver means nobody waits for the outcome any more.
pub(crate) fn answer(reply: Reply, result: Result<(), HelperError>) {
    let _ = reply.send(result);
}
