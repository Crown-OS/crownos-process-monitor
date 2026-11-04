use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::JoinHandle;

use monitor_core::util::time::unix_now;

use crate::batch::RollupBatch;
use crate::error::{HistoryError, HistoryResult};
use crate::retention::Retention;
use crate::sample::KernelCounters;
use crate::store::{GapOutcome, HistoryStore, StoredAlert};

/// Minutes of batches buffered while the disk is slow; beyond that new
/// batches are dropped (and counted) so the sampler never blocks.
const QUEUE_CAPACITY: usize = 32;
const THREAD_NAME: &str = "history-writer";

enum Command {
    Batch(Box<RollupBatch>),
    Checkpoint(KernelCounters),
    SessionStart(KernelCounters, SyncSender<HistoryResult<GapOutcome>>),
    SetRetention(Retention),
    SaveAlert(Box<StoredAlert>),
    DeleteAll(SyncSender<HistoryResult<()>>),
    Shutdown,
}

#[derive(Debug, Default)]
struct WriterCounters {
    dropped_batches: AtomicU64,
    failed_writes: AtomicU64,
}

/// Owns the write connection on its own thread. The thread sleeps in
/// `recv` between commands, so it causes no wakeups between minutes.
pub struct HistoryWriter {
    commands: SyncSender<Command>,
    thread: Option<JoinHandle<HistoryResult<()>>>,
    counters: Arc<WriterCounters>,
}

impl HistoryWriter {
    /// Opens (creating and migrating) the database on the calling thread so
    /// errors surface here, then moves it to the writer thread.
    pub fn spawn(path: &Path, retention: Retention) -> HistoryResult<Self> {
        let store = HistoryStore::open(path, retention)?;
        let (commands, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let counters = Arc::new(WriterCounters::default());
        let thread_counters = Arc::clone(&counters);
        let thread = std::thread::Builder::new()
            .name(THREAD_NAME.into())
            .spawn(move || run(store, &receiver, &thread_counters))?;
        Ok(Self {
            commands,
            thread: Some(thread),
            counters,
        })
    }

    /// Queues a finished minute without blocking. Returns `false` when the
    /// queue was full or the writer stopped; the batch is then counted as
    /// dropped.
    pub fn submit(&self, batch: RollupBatch) -> bool {
        let queued = self.try_command(Command::Batch(Box::new(batch)));
        if !queued {
            self.counters
                .dropped_batches
                .fetch_add(1, Ordering::Relaxed);
        }
        queued
    }

    /// Persists since-boot counters; call right after the minute's `submit`.
    /// Never blocks: a skipped checkpoint is replaced by the next one.
    pub fn checkpoint(&self, counters: KernelCounters) -> bool {
        self.try_command(Command::Checkpoint(counters))
    }

    /// Starts a session and records the gap since the previous one (§6.4).
    /// The receiver yields what kind of gap was found.
    pub fn record_session_start(
        &self,
        counters: KernelCounters,
    ) -> HistoryResult<Receiver<HistoryResult<GapOutcome>>> {
        let (reply, outcome) = mpsc::sync_channel(1);
        self.send(Command::SessionStart(counters, reply))?;
        Ok(outcome)
    }

    pub fn set_retention(&self, retention: Retention) -> HistoryResult<()> {
        self.send(Command::SetRetention(retention))
    }

    pub fn save_alert(&self, alert: StoredAlert) -> HistoryResult<()> {
        self.send(Command::SaveAlert(Box::new(alert)))
    }

    /// Clears all history; the receiver yields the result once done.
    pub fn delete_all(&self) -> HistoryResult<Receiver<HistoryResult<()>>> {
        let (reply, done) = mpsc::sync_channel(1);
        self.send(Command::DeleteAll(reply))?;
        Ok(done)
    }

    pub fn dropped_batches(&self) -> u64 {
        self.counters.dropped_batches.load(Ordering::Relaxed)
    }

    pub fn failed_writes(&self) -> u64 {
        self.counters.failed_writes.load(Ordering::Relaxed)
    }

    /// Writes everything queued, closes the database and joins the thread.
    pub fn shutdown(mut self) -> HistoryResult<()> {
        self.stop()
    }

    fn stop(&mut self) -> HistoryResult<()> {
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        let _ = self.commands.send(Command::Shutdown);
        thread.join().map_err(|_| HistoryError::WriterStopped)?
    }

    fn send(&self, command: Command) -> HistoryResult<()> {
        self.commands
            .send(command)
            .map_err(|_| HistoryError::WriterStopped)
    }

    fn try_command(&self, command: Command) -> bool {
        self.commands.try_send(command).is_ok()
    }
}

impl Drop for HistoryWriter {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn run(
    mut store: HistoryStore,
    commands: &Receiver<Command>,
    counters: &WriterCounters,
) -> HistoryResult<()> {
    let count_failure = |result: HistoryResult<()>| {
        if result.is_err() {
            counters.failed_writes.fetch_add(1, Ordering::Relaxed);
        }
    };
    for command in commands {
        match command {
            Command::Batch(batch) => count_failure(store.write_batch(&batch)),
            Command::Checkpoint(counters) => count_failure(store.checkpoint(&counters)),
            Command::SessionStart(counters, reply) => {
                let _ = reply.send(store.start_session(&counters));
            }
            Command::SetRetention(retention) => store.set_retention(retention),
            Command::SaveAlert(alert) => count_failure(store.save_alert(&alert)),
            Command::DeleteAll(reply) => {
                let _ = reply.send(store.delete_all(unix_now()));
            }
            Command::Shutdown => break,
        }
    }
    store.close()
}
