//! The engine against the real helper service on a private `dbus-daemon`
//! (skipped when it is not installed), authorized by an allow-list for the
//! current user.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as Process, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use monitor_core::{Interest, ProcessKey};
use monitor_engine::helper::auth::StaticAuthorizer;
use monitor_engine::helper::{Capability, HelperAvailability, HelperState, SignalKind, serve};
use monitor_engine::{
    Command, Engine, EngineConfig, HelperAction, HelperConnection, Notifications, Snapshot,
};
use tokio::sync::watch;

const PATIENCE: Duration = Duration::from_secs(10);

struct PrivateBus {
    daemon: Child,
    directory: PathBuf,
    address: String,
}

impl PrivateBus {
    fn start() -> Option<Self> {
        let daemon = std::env::var_os("PATH")?
            .to_str()?
            .split(':')
            .map(|directory| Path::new(directory).join("dbus-daemon"))
            .find(|candidate| candidate.is_file())?;
        let directory =
            std::env::temp_dir().join(format!("monitor-engine-helper-{}", std::process::id()));
        std::fs::create_dir_all(&directory).ok()?;
        let socket = directory.join("bus");
        let config = directory.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path={}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>"#,
                socket.display()
            ),
        )
        .ok()?;
        let daemon = Process::new(daemon)
            .arg(format!("--config-file={}", config.display()))
            .arg("--nofork")
            .stdout(Stdio::null())
            .spawn()
            .ok()?;
        let deadline = Instant::now() + PATIENCE;
        while !socket.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        Some(Self {
            daemon,
            address: format!("unix:path={}", socket.display()),
            directory,
        })
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// Serves the helper on its own thread for the rest of the test process.
fn spawn_service(address: String) {
    let uid = std::fs::metadata("/proc/self").map_or(0, |metadata| metadata.uid());
    let policy = format!(
        "read-all-processes = uid:{uid}\nread-gpu-counters = uid:{uid}\nsignal-other-users = uid:{uid}"
    );
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            let Ok(authorizer) = StaticAuthorizer::parse(&policy) else {
                return;
            };
            let Ok(state) = HelperState::new(authorizer) else {
                return;
            };
            let builder = zbus::connection::Builder::address(address.as_str());
            let Ok(connection) = builder.build().await else {
                return;
            };
            if serve(&connection, Arc::new(state)).await.is_ok() {
                std::future::pending::<()>().await;
            }
        });
    });
}

fn start_ticks(pid: u32) -> Option<u64> {
    let stat = std::fs::read(format!("/proc/{pid}/stat")).ok()?;
    monitor_engine::system::parse_stat(&stat).map(|(_, stat)| stat.start_ticks)
}

fn block_on<T>(future: impl Future<Output = T>) -> T {
    match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime.block_on(future),
        Err(error) => panic!("no runtime: {error}"),
    }
}

async fn until(
    snapshots: &mut watch::Receiver<Arc<Snapshot>>,
    condition: impl Fn(&Snapshot) -> bool,
) -> Arc<Snapshot> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let latest = snapshots.borrow_and_update().clone();
        if condition(&latest) {
            return latest;
        }
        match tokio::time::timeout_at(deadline, snapshots.changed()).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => panic!("the engine stopped publishing"),
            Err(_) => panic!("condition not met within {PATIENCE:?}"),
        }
    }
}

#[test]
fn reads_and_acts_through_the_helper() {
    let Some(bus) = PrivateBus::start() else {
        eprintln!("dbus-daemon is not installed; skipping");
        return;
    };
    spawn_service(bus.address.clone());
    let config = EngineConfig {
        interest: Interest::default(),
        history_path: Some(bus.directory.join("history.sqlite")),
        notifications: Notifications::Off,
        helper: HelperConnection::Address(bus.address.clone()),
        ..EngineConfig::default()
    };
    let engine = match Engine::start(config) {
        Ok(engine) => engine,
        Err(error) => panic!("engine did not start: {error}"),
    };
    let mut snapshots = engine.snapshots();
    let mut outcomes = engine.helper_outcomes();
    let Ok(mut child) = Process::new("sleep").arg("30").spawn() else {
        return;
    };
    let Some(child_start) = start_ticks(child.id()) else {
        panic!("child stat unreadable");
    };
    block_on(async {
        let ready = until(&mut snapshots, |snapshot| {
            snapshot.helper.serves(Capability::ReadAllProcesses)
        })
        .await;
        assert_eq!(ready.helper.availability, HelperAvailability::Available);
        assert!(
            !ready.helper.capabilities.contains(&Capability::ReadEnergy),
            "refused energy reads are not offered: {:?}",
            ready.helper.capabilities
        );

        until(&mut snapshots, |snapshot| {
            snapshot.gpu.devices.is_empty()
                || snapshot.gpu.process_coverage == monitor_core::Coverage::Full
        })
        .await;

        let key = ProcessKey {
            pid: monitor_core::Pid::new(child.id()),
            start_ticks: child_start,
        };
        assert!(
            engine
                .send(Command::HelperSignal {
                    key,
                    signal: SignalKind::Term,
                })
                .is_ok()
        );
        let outcome = tokio::time::timeout(PATIENCE, outcomes.recv()).await;
        let Ok(Ok(outcome)) = outcome else {
            panic!("no helper outcome: {outcome:?}");
        };
        assert_eq!(
            outcome.action,
            HelperAction::Signal {
                key,
                signal: SignalKind::Term
            }
        );
        assert_eq!(outcome.result, Ok(()));

        assert!(engine.send(Command::RestoreFans).is_ok());
        let refused = tokio::time::timeout(PATIENCE, outcomes.recv()).await;
        assert!(
            matches!(refused, Ok(Ok(ref outcome)) if outcome.result.is_err()),
            "fan control is not allowed by the policy: {refused:?}"
        );
    });
    let exited = child.wait().map(|status| !status.success());
    assert!(exited.is_ok_and(|killed| killed), "SIGTERM ended the child");
    assert!(engine.shutdown().is_ok());
}
