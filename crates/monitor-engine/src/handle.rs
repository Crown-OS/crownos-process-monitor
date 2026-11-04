use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use monitor_alerts::AlertRecord;
use monitor_core::util::time::unix_now;
use monitor_core::{Interest, ProcessKey, SourceError, SourceResult};
use monitor_history::{HistoryReader, HistoryStore, Span};
use monitor_system::ProcessDetails;
use tokio::sync::{broadcast, oneshot, watch};

use crate::alerting::{Alerting, QuietFile, from_stored};
use crate::command::Message;
use crate::drives::DRIVE_OUTCOME_BACKLOG;
use crate::privileged::OUTCOME_BACKLOG;
use crate::recording::Recorder;
use crate::sampler::{self, Sampler, Startup};
use crate::util::local_time::utc_offset_seconds;
use crate::{Command, DriveOutcome, EngineConfig, EngineError, HelperOutcome, Snapshot};

const THREAD_NAME: &str = "monitor-sampler";
const ENGINE: &str = "the monitoring engine";

pub struct Engine;

impl Engine {
    /// Opens history, loads past alerts and starts the sampler thread. The
    /// first snapshot follows once the collectors are open; a collector or
    /// history failure only marks its domain unavailable.
    pub fn start(config: EngineConfig) -> Result<EngineHandle, EngineError> {
        let history_path: Option<Arc<Path>> = config
            .history_path
            .or_else(|| HistoryStore::default_path().ok())
            .map(PathBuf::into);
        let opened = history_path
            .as_deref()
            .map(|path| Recorder::open(path, config.settings.retention));
        let (recorder, history_failure) = match opened {
            Some(Ok(recorder)) => (Some(recorder), None),
            Some(Err(error)) => (None, Some(error.to_string())),
            None => (None, Some(EngineError::NoHistory.to_string())),
        };
        let past = history_path
            .as_deref()
            .filter(|_| recorder.is_some())
            .map(past_alerts)
            .unwrap_or_default();
        let quiet_file = history_path.as_deref().map(QuietFile::beside);
        let (outcomes, _) = broadcast::channel(OUTCOME_BACKLOG);
        let (drive_outcomes, _) = broadcast::channel(DRIVE_OUTCOME_BACKLOG);
        let startup = Startup {
            alerting: Alerting::new(&config.settings, past, config.notifications, quiet_file),
            interest: config.interest,
            settings: config.settings,
            recorder,
            history_failure,
            helper: config.helper,
            outcomes: outcomes.clone(),
            drive_outcomes: drive_outcomes.clone(),
        };
        let (publisher, snapshots) = watch::channel(Arc::new(Snapshot::default()));
        let (messages, inbox) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name(THREAD_NAME.into())
            .spawn(move || sampler::run(Sampler::new(startup, publisher), &inbox))
            .map_err(EngineError::Spawn)?;
        Ok(EngineHandle {
            control: EngineControl {
                messages,
                snapshots,
                history_path,
                outcomes,
                drive_outcomes,
            },
            thread: Some(thread),
        })
    }
}

fn past_alerts(path: &Path) -> Vec<AlertRecord> {
    HistoryReader::open(path)
        .and_then(|reader| reader.alerts(Span::last_30_days(unix_now(), utc_offset_seconds())))
        .map(|stored| stored.into_iter().filter_map(from_stored).collect())
        .unwrap_or_default()
}

/// A cheap, cloneable remote for the running engine. It never stops the
/// engine: the [`EngineHandle`] owner decides when sampling ends.
#[derive(Clone, Debug)]
pub struct EngineControl {
    messages: Sender<Message>,
    snapshots: watch::Receiver<Arc<Snapshot>>,
    history_path: Option<Arc<Path>>,
    outcomes: broadcast::Sender<HelperOutcome>,
    drive_outcomes: broadcast::Sender<DriveOutcome>,
}

impl EngineControl {
    /// Await `changed()` on it: a slow reader skips ticks, it never queues them.
    pub fn snapshots(&self) -> watch::Receiver<Arc<Snapshot>> {
        self.snapshots.clone()
    }

    /// Samples at once (at most four times a second) when anything changed.
    /// `visible_interval` is ignored: [`crate::EngineSettings`] decides it.
    pub fn set_interest(&self, interest: Interest) -> Result<(), EngineError> {
        self.post(Message::Interest(interest))
    }

    pub fn send(&self, command: Command) -> Result<(), EngineError> {
        self.post(Message::Command(command))
    }

    pub fn history_path(&self) -> Option<&Path> {
        self.history_path.as_deref()
    }

    /// Outcomes of helper actions ([`Command::HelperSignal`],
    /// [`Command::HelperRenice`], [`Command::SetFan`],
    /// [`Command::RestoreFans`]), each once the helper answered, which may
    /// follow an authentication dialog. Subscribe before sending; a
    /// subscriber more than 16 outcomes behind misses the oldest.
    pub fn helper_outcomes(&self) -> broadcast::Receiver<HelperOutcome> {
        self.outcomes.subscribe()
    }

    /// Outcomes of [`Command::Drive`], each once UDisks2 answered, which may
    /// follow a polkit dialog. Subscribe before sending.
    pub fn drive_outcomes(&self) -> broadcast::Receiver<DriveOutcome> {
        self.drive_outcomes.subscribe()
    }

    /// Inspector details of `key`, gathered on the sampler thread between
    /// ticks with its cached desktop index and app grouping. Await the
    /// answer off the UI thread; it is `Unavailable` once the engine stopped.
    pub fn inspect(
        &self,
        key: ProcessKey,
    ) -> impl Future<Output = SourceResult<ProcessDetails>> + Send + use<> {
        let (reply, answer) = oneshot::channel();
        let posted = self.post(Message::Inspect { key, reply });
        async move {
            posted.map_err(|_| SourceError::Unavailable(ENGINE))?;
            answer
                .await
                .unwrap_or(Err(SourceError::Unavailable(ENGINE)))
        }
    }

    /// A read-only connection for range queries; run them on a blocking task.
    pub fn history_reader(&self) -> Result<HistoryReader, EngineError> {
        let path = self.history_path().ok_or(EngineError::NoHistory)?;
        Ok(HistoryReader::open(path)?)
    }

    fn post(&self, message: Message) -> Result<(), EngineError> {
        self.messages
            .send(message)
            .map_err(|_| EngineError::Stopped)
    }
}

/// Owns the sampler thread. Dropping it writes the unfinished history
/// minute, closes the database and joins the thread.
#[derive(Debug)]
pub struct EngineHandle {
    control: EngineControl,
    thread: Option<JoinHandle<()>>,
}

impl EngineHandle {
    pub fn control(&self) -> EngineControl {
        self.control.clone()
    }

    pub fn snapshots(&self) -> watch::Receiver<Arc<Snapshot>> {
        self.control.snapshots()
    }

    pub fn set_interest(&self, interest: Interest) -> Result<(), EngineError> {
        self.control.set_interest(interest)
    }

    pub fn send(&self, command: Command) -> Result<(), EngineError> {
        self.control.send(command)
    }

    pub fn history_path(&self) -> Option<&Path> {
        self.control.history_path()
    }

    pub fn helper_outcomes(&self) -> broadcast::Receiver<HelperOutcome> {
        self.control.helper_outcomes()
    }

    pub fn history_reader(&self) -> Result<HistoryReader, EngineError> {
        self.control.history_reader()
    }

    /// Stops sampling and waits until history is written.
    pub fn shutdown(mut self) -> Result<(), EngineError> {
        self.stop()
    }

    fn stop(&mut self) -> Result<(), EngineError> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        let _ = self.control.send(Command::Shutdown);
        thread.join().map_err(|_| EngineError::Stopped)
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
