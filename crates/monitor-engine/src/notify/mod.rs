mod text;

use std::collections::HashMap;

use monitor_alerts::Alert;
use monitor_core::util::runtime_thread::spawn_runtime_thread;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{Connection, Variant};

const THREAD_NAME: &str = "monitor-notify";
const APP_NAME: &str = "Process Monitor";
const APP_ICON: &str = "utilities-system-monitor";
const DESTINATION: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const NOTIFY: &str = "Notify";
const DEFAULT_TIMEOUT: i32 = -1;

struct Notification {
    summary: String,
    body: String,
}

/// Desktop notifications sent from a background thread, so a slow or
/// missing notification daemon never delays a tick.
pub(crate) struct Notifier {
    queue: UnboundedSender<Notification>,
}

impl Notifier {
    /// The thread exits once the notifier is dropped.
    pub fn spawn() -> Option<Self> {
        let (queue, pending) = unbounded_channel();
        spawn_runtime_thread(THREAD_NAME, move || deliver(pending)).ok()?;
        Some(Self { queue })
    }

    pub fn alert(&self, alert: &Alert) {
        let (summary, body) = text::describe(alert);
        let _ = self.queue.send(Notification { summary, body });
    }
}

async fn deliver(mut pending: UnboundedReceiver<Notification>) {
    let mut connection: Option<Connection> = None;
    while let Some(notification) = pending.recv().await {
        if connection.is_none() {
            connection = Connection::session().await.ok();
        }
        let Some(bus) = &connection else { continue };
        if send(bus, &notification).await.is_err() {
            connection = None;
        }
    }
}

async fn send(bus: &Connection, notification: &Notification) -> zbus::Result<()> {
    let actions: &[&str] = &[];
    let hints: HashMap<&str, Variant<'_>> = HashMap::new();
    bus.call_method(
        Some(DESTINATION),
        PATH,
        Some(DESTINATION),
        NOTIFY,
        &(
            APP_NAME,
            0_u32,
            APP_ICON,
            notification.summary.as_str(),
            notification.body.as_str(),
            actions,
            hints,
            DEFAULT_TIMEOUT,
        ),
    )
    .await
    .map(drop)
}
