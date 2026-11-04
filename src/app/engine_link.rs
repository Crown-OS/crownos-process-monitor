use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use anyhow::{Context, Result};
use crownui::prelude::{Cx, Runtime};
use monitor_core::{AppKey, ProcessKey, SourceError, SourceResult};
use monitor_engine::history::{HistoryReader, HistoryResult};
use monitor_engine::system::ProcessDetails;
use monitor_engine::{Command, DriveOutcome, EngineControl, HelperOutcome};
use tokio::sync::broadcast;

const NO_ENGINE: &str = "the monitoring engine";

struct Shared {
    control: Option<EngineControl>,
    history_path: Option<Arc<Path>>,
    reader: Mutex<Option<HistoryReader>>,
}

/// The window's one handle on the engine: commands, action outcomes, inspection, plus one
/// cached read-only history connection shared by every query. Provided once at the root;
/// pages find it in their environment, so headless builds without an engine simply have none.
#[derive(Clone)]
pub struct EngineLink(Arc<Shared>);

impl EngineLink {
    pub fn new(control: EngineControl) -> Self {
        let history_path = control.history_path().map(Arc::from);
        Self(Arc::new(Shared {
            control: Some(control),
            history_path,
            reader: Mutex::new(None),
        }))
    }

    /// Reads an existing history database without an engine.
    #[cfg(test)]
    pub fn history_only(path: impl Into<std::path::PathBuf>) -> Self {
        Self(Arc::new(Shared {
            control: None,
            history_path: Some(Arc::from(path.into())),
            reader: Mutex::new(None),
        }))
    }

    pub fn provide(self, cx: &mut Cx) {
        cx.provide(self);
    }

    pub fn find(runtime: &Runtime) -> Option<Self> {
        runtime
            .environment::<Self>()
            .map(|link| link.with_untracked(runtime, Self::clone))
    }

    /// Sends `command` to the engine; without one it is dropped.
    pub fn send(&self, command: Command) {
        if let Some(control) = &self.0.control
            && control.send(command).is_err()
        {
            eprintln!("process-monitor: the engine stopped; a command was dropped");
        }
    }

    /// Inspector details read on the sampler thread with its cached desktop index.
    pub fn inspect(
        &self,
        key: ProcessKey,
    ) -> impl Future<Output = SourceResult<ProcessDetails>> + Send + use<> {
        let answer = self.0.control.as_ref().map(|control| control.inspect(key));
        async move {
            match answer {
                Some(answer) => answer.await,
                None => Err(SourceError::Unavailable(NO_ENGINE)),
            }
        }
    }

    /// Outcomes of helper actions (signals, renice, fans); subscribe before sending.
    pub fn helper_outcomes(&self) -> Option<broadcast::Receiver<HelperOutcome>> {
        self.0.control.as_ref().map(EngineControl::helper_outcomes)
    }

    /// Outcomes of unmount, eject and power off; subscribe before sending.
    pub fn drive_outcomes(&self) -> Option<broadcast::Receiver<DriveOutcome>> {
        self.0.control.as_ref().map(EngineControl::drive_outcomes)
    }

    /// The apps of the latest tick, by name, for pickers.
    pub fn running_apps(&self) -> Vec<(AppKey, Arc<str>)> {
        let Some(control) = &self.0.control else {
            return Vec::new();
        };
        let snapshot = control.snapshots().borrow().clone();
        let mut apps: Vec<_> = snapshot
            .processes
            .apps
            .iter()
            .map(|app| (app.key.clone(), Arc::clone(&app.name)))
            .collect();
        apps.sort_by(|first, second| first.1.cmp(&second.1));
        apps
    }

    /// Runs `query` on the cached connection, opening it first if needed. Blocks on SQLite:
    /// call it from `spawn_blocking`. A failed query drops the connection so the next one
    /// reopens it (the database may have been deleted and recreated).
    pub fn read<T>(&self, query: impl FnOnce(&HistoryReader) -> HistoryResult<T>) -> Result<T> {
        let mut cached = self.lock_reader();
        let reader = match cached.take() {
            Some(reader) => reader,
            None => self.open_reader()?,
        };
        let result = query(&reader).context("History query failed");
        if result.is_ok() {
            *cached = Some(reader);
        }
        result
    }

    fn open_reader(&self) -> Result<HistoryReader> {
        if let Some(control) = &self.0.control {
            return control
                .history_reader()
                .context("Cannot open the history database");
        }
        let path = self
            .0
            .history_path
            .as_deref()
            .context("No history database is configured")?;
        HistoryReader::open(path).context("Cannot open the history database")
    }

    fn lock_reader(&self) -> MutexGuard<'_, Option<HistoryReader>> {
        self.0.reader.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
