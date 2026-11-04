//! UDisks2 on a background thread: SMART health refreshes and the user's
//! unmount / eject / power-off requests. The sampler only reads the latest
//! published [`HealthTable`].

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use zbus::Connection;

use super::health::DriveHealth;
use crate::IoError;
use actions::{DriveAction, drive_action, unmount};
use monitor_core::util::latest::Latest;
use monitor_core::util::runtime_thread::spawn_runtime_thread;
use objects::ObjectIndex;
use smart::refresh;

mod actions;
mod objects;
mod smart;

const SERVICE: &str = "org.freedesktop.UDisks2";
const THREAD_NAME: &str = "monitor-udisks";

/// Resolves once UDisks2 (and polkit) answered the request.
pub type RequestReceiver = oneshot::Receiver<Result<(), IoError>>;

type Reply = oneshot::Sender<Result<(), IoError>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TableState {
    #[default]
    Pending,
    Ready,
    Unavailable,
}

/// Health per whole-disk kernel name.
#[derive(Debug, Default)]
pub(crate) struct HealthTable {
    pub state: TableState,
    pub drives: HashMap<Arc<str>, DriveHealth>,
}

impl HealthTable {
    fn unavailable() -> Self {
        Self {
            state: TableState::Unavailable,
            drives: HashMap::new(),
        }
    }

    pub(crate) fn health(&self, disk: &str) -> DriveHealth {
        match self.state {
            TableState::Pending => DriveHealth::Pending,
            TableState::Unavailable => DriveHealth::Unavailable,
            TableState::Ready => self
                .drives
                .get(disk)
                .cloned()
                .unwrap_or(DriveHealth::Unavailable),
        }
    }
}

enum Command {
    Refresh,
    Unmount { device: Arc<str>, reply: Reply },
    Eject { device: Arc<str>, reply: Reply },
    PowerOff { device: Arc<str>, reply: Reply },
}

impl Command {
    fn fail(self, error: IoError) {
        match self {
            Self::Refresh => {}
            Self::Unmount { reply, .. }
            | Self::Eject { reply, .. }
            | Self::PowerOff { reply, .. } => {
                let _ = reply.send(Err(error));
            }
        }
    }
}

/// Handle to the UDisks2 worker. Cheap to clone; the UI keeps one to issue
/// requests. Requests that need authorization show the polkit dialog.
#[derive(Clone, Debug)]
pub struct UDisks {
    commands: mpsc::UnboundedSender<Command>,
}

impl std::fmt::Debug for Command {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Refresh => "Refresh",
            Self::Unmount { .. } => "Unmount",
            Self::Eject { .. } => "Eject",
            Self::PowerOff { .. } => "PowerOff",
        })
    }
}

impl UDisks {
    pub(crate) fn spawn(table: Arc<Latest<HealthTable>>) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let worker_table = Arc::clone(&table);
        if spawn_runtime_thread(THREAD_NAME, move || serve(receiver, worker_table)).is_err() {
            table.publish(HealthTable::unavailable());
        }
        Self { commands }
    }

    /// Re-reads SMART data of every drive in the background.
    pub fn refresh_health(&self) {
        let _ = self.commands.send(Command::Refresh);
    }

    /// Unmounts the filesystem on `block_device` (kernel name, e.g. `sdb1`).
    pub fn unmount(&self, block_device: &str) -> RequestReceiver {
        self.request(
            |device, reply| Command::Unmount { device, reply },
            block_device,
        )
    }

    /// Ejects the medium of the drive behind `block_device` (optical, SD).
    pub fn eject(&self, block_device: &str) -> RequestReceiver {
        self.request(
            |device, reply| Command::Eject { device, reply },
            block_device,
        )
    }

    /// Powers off the drive behind `block_device`, so a USB disk can be
    /// unplugged safely. Unmount its filesystems first.
    pub fn power_off(&self, block_device: &str) -> RequestReceiver {
        self.request(
            |device, reply| Command::PowerOff { device, reply },
            block_device,
        )
    }

    fn request(
        &self,
        command: impl FnOnce(Arc<str>, Reply) -> Command,
        block_device: &str,
    ) -> RequestReceiver {
        let (reply, receiver) = oneshot::channel();
        if let Err(rejected) = self.commands.send(command(Arc::from(block_device), reply)) {
            rejected.0.fail(IoError::WorkerStopped);
        }
        receiver
    }
}

async fn serve(mut commands: mpsc::UnboundedReceiver<Command>, table: Arc<Latest<HealthTable>>) {
    let connection = Connection::system().await.ok();
    let mut index = ObjectIndex::default();
    while let Some(command) = commands.recv().await {
        let Some(connection) = connection.as_ref() else {
            table.publish(HealthTable::unavailable());
            command.fail(IoError::Unavailable);
            continue;
        };
        match command {
            Command::Refresh => table.publish(refresh(connection, &mut index).await),
            Command::Unmount { device, reply } => {
                let outcome = unmount(connection, &mut index, &device).await;
                let _ = reply.send(outcome);
            }
            Command::Eject { device, reply } => {
                let outcome =
                    drive_action(connection, &mut index, &device, DriveAction::Eject).await;
                let _ = reply.send(outcome);
            }
            Command::PowerOff { device, reply } => {
                let outcome =
                    drive_action(connection, &mut index, &device, DriveAction::PowerOff).await;
                let _ = reply.send(outcome);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn missing_drives_are_unavailable_once_ready() {
        let table = HealthTable {
            state: TableState::Ready,
            drives: HashMap::new(),
        };
        assert_eq!(table.health("sda"), DriveHealth::Unavailable);
        assert_eq!(HealthTable::default().health("sda"), DriveHealth::Pending);
    }

    #[test]
    #[ignore = "needs the system bus and UDisks2"]
    fn reads_live_smart_data() {
        let table = Arc::new(Latest::<HealthTable>::default());
        let udisks = UDisks::spawn(Arc::clone(&table));
        udisks.refresh_health();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(current) = table
                .try_get()
                .filter(|current| current.state != TableState::Pending)
            {
                for (name, health) in &current.drives {
                    println!("{name}: {:?} {health:?}", health.verdict());
                }
                assert_eq!(current.state, TableState::Ready);
                assert!(matches!(current.health("nvme0n1"), DriveHealth::Nvme(_)));
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("UDisks2 did not answer within 10 s");
    }
}
