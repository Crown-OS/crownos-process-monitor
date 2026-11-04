//! power-profiles-daemon and logind inhibitors, queried on a dedicated
//! thread with its own single-threaded runtime so a slow bus never stalls a
//! sample. `sample()` only ever `try_lock`s the shared state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use monitor_core::util::runtime_thread::spawn_runtime_thread;
use monitor_core::util::time::wall_unix_seconds;
use monitor_core::{Coverage, Pid, Uid};
use tokio::sync::oneshot;
use zbus::{Connection, Proxy};

use super::snapshot::{InhibitMode, Inhibitor};

const REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const THREAD_NAME: &str = "monitor-power-dbus";
const PROFILE_SERVICES: [(&str, &str); 2] = [
    (
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
    ),
    ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles"),
];
const LOGIN1: &str = "org.freedesktop.login1";
const LOGIN1_PATH: &str = "/org/freedesktop/login1";
const LOGIN1_MANAGER: &str = "org.freedesktop.login1.Manager";

type RawInhibitor = (String, String, String, String, u32, u32);

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DbusState {
    pub(crate) generation: u64,
    pub(crate) daemon_profile: Option<Arc<str>>,
    pub(crate) inhibitors: Arc<[Inhibitor]>,
    pub(crate) coverage: Coverage,
}

#[derive(Debug)]
pub(crate) struct DbusWatcher {
    shared: Arc<Mutex<DbusState>>,
    seen_generation: u64,
    _stop: Option<oneshot::Sender<()>>,
}

impl DbusWatcher {
    pub(crate) fn spawn() -> Self {
        let shared = Arc::new(Mutex::new(DbusState {
            coverage: Coverage::Unavailable,
            ..DbusState::default()
        }));
        let (stop, stopped) = oneshot::channel();
        let worker_state = Arc::clone(&shared);
        let spawned =
            spawn_runtime_thread(THREAD_NAME, move || refresh_forever(worker_state, stopped));
        Self {
            shared,
            seen_generation: 0,
            _stop: spawned.is_ok().then_some(stop),
        }
    }

    /// The newest state if it changed since the last call. Never blocks.
    pub(crate) fn latest(&mut self) -> Option<DbusState> {
        let state = self.shared.try_lock().ok()?;
        if state.generation == self.seen_generation {
            return None;
        }
        self.seen_generation = state.generation;
        Some(state.clone())
    }
}

async fn refresh_forever(shared: Arc<Mutex<DbusState>>, mut stopped: oneshot::Receiver<()>) {
    let mut bus = Bus::default();
    let mut generation = 0;
    loop {
        let mut state = bus.query().await;
        generation += 1;
        state.generation = generation;
        if let Ok(mut shared) = shared.lock() {
            *shared = state;
        }
        tokio::select! {
            _ = &mut stopped => break,
            () = tokio::time::sleep(REFRESH_INTERVAL) => {}
        }
    }
}

#[derive(Default)]
struct Bus {
    connection: Option<Connection>,
    profiles: Option<Proxy<'static>>,
    first_seen: HashMap<RawInhibitor, u64>,
}

impl Bus {
    async fn query(&mut self) -> DbusState {
        if self.connection.is_none() {
            self.connection = Connection::system().await.ok();
            self.profiles = None;
        }
        let Some(connection) = self.connection.clone() else {
            return DbusState {
                coverage: Coverage::Unavailable,
                ..DbusState::default()
            };
        };
        if self.profiles.is_none() {
            self.profiles = profiles_proxy(&connection).await;
        }
        let daemon_profile = match &self.profiles {
            Some(proxy) => proxy
                .get_property::<String>("ActiveProfile")
                .await
                .ok()
                .map(Arc::from),
            None => None,
        };
        let inhibitors = match list_inhibitors(&connection).await {
            Ok(raw) => self.inhibitors(raw),
            Err(zbus::Error::InputOutput(_)) => {
                self.connection = None;
                Arc::from([])
            }
            Err(_) => Arc::from([]),
        };
        DbusState {
            generation: 0,
            daemon_profile,
            inhibitors,
            coverage: Coverage::Full,
        }
    }

    fn inhibitors(&mut self, raw: Vec<RawInhibitor>) -> Arc<[Inhibitor]> {
        let now = wall_unix_seconds();
        self.first_seen.retain(|key, _| raw.contains(key));
        raw.into_iter()
            .map(|key| {
                let first_seen_unix = *self.first_seen.entry(key.clone()).or_insert(now);
                let (what, who, why, mode, uid, pid) = key;
                Inhibitor {
                    what: Arc::from(what),
                    who: Arc::from(who),
                    why: Arc::from(why),
                    mode: match mode.as_str() {
                        "block" => InhibitMode::Block,
                        "delay" => InhibitMode::Delay,
                        _ => InhibitMode::Other,
                    },
                    uid: Uid::new(uid),
                    pid: Pid::new(pid),
                    first_seen_unix,
                }
            })
            .collect()
    }
}

async fn profiles_proxy(connection: &Connection) -> Option<Proxy<'static>> {
    for (service, path) in PROFILE_SERVICES {
        let Ok(proxy) = Proxy::new(connection, service, path, service).await else {
            continue;
        };
        if proxy.get_property::<String>("ActiveProfile").await.is_ok() {
            return Some(proxy);
        }
    }
    None
}

async fn list_inhibitors(connection: &Connection) -> zbus::Result<Vec<RawInhibitor>> {
    let reply = connection
        .call_method(
            Some(LOGIN1),
            LOGIN1_PATH,
            Some(LOGIN1_MANAGER),
            "ListInhibitors",
            &(),
        )
        .await?;
    reply.body().deserialize()
}
