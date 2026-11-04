use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use monitor_helper::auth::{self, Authorizer};
use monitor_helper::{HelperState, serve};
use tokio::signal::unix::{SignalKind, signal};
use zbus::fdo::{DBusProxy, NameOwnerChangedStream};

fn main() -> Result<()> {
    if !rustix::process::geteuid().is_root() {
        bail!(
            "monitor-helper must run as root; the system bus starts it on demand \
             for org.crownos.ProcessMonitor1"
        );
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?
        .block_on(run())
}

async fn run() -> Result<()> {
    let connection = zbus::Connection::system()
        .await
        .context("cannot connect to the system bus")?;
    let authorizer = auth::selected(&connection)
        .await
        .context("cannot set up authorization")?;
    let state = Arc::new(HelperState::new(authorizer).context("cannot open /proc")?);
    let departures = DBusProxy::new(&connection)
        .await?
        .receive_name_owner_changed()
        .await
        .context("cannot watch bus clients")?;
    serve(&connection, Arc::clone(&state))
        .await
        .context("cannot publish org.crownos.ProcessMonitor1")?;
    let outcome = serve_until_idle(&state, departures).await;
    state
        .restore_all_fans()
        .context("restoring automatic fan control failed")?;
    outcome
}

/// Returns after SIGTERM/SIGINT, or once the last client left and the
/// grace period passed without a new one.
async fn serve_until_idle<A: Authorizer>(
    state: &HelperState<A>,
    mut departures: NameOwnerChangedStream,
) -> Result<()> {
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    loop {
        let deadline = state.clients().exit_deadline();
        tokio::select! {
            () = sleep_until(deadline) => {
                if state.clients().should_exit() {
                    return Ok(());
                }
            }
            () = state.clients().arrival() => {}
            change = departures.next() => {
                let Some(change) = change else {
                    bail!("lost the system bus");
                };
                let Ok(arguments) = change.args() else {
                    continue;
                };
                if arguments.new_owner().is_none()
                    && let Err(error) = state.client_departed(arguments.name())
                {
                    eprintln!("monitor-helper: restoring fans of {} failed: {error}", arguments.name());
                }
            }
            _ = terminate.recv() => return Ok(()),
            _ = interrupt.recv() => return Ok(()),
        }
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending().await,
    }
}
