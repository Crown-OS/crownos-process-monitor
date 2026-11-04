mod counters;
mod samples;

pub use self::samples::history_engine;
pub(crate) use self::samples::max_percent;

use std::path::Path;

use monitor_history::{Accumulator, HistoryResult, HistoryWriter, Retention, StoredAlert};

use self::counters::KernelCounterReader;
use self::samples::SampleBuffers;
use crate::Snapshot;

/// Folds every tick into minute rollups and hands them to the history
/// writer thread; never blocks the sampler.
pub(crate) struct Recorder {
    writer: HistoryWriter,
    accumulator: Accumulator,
    samples: SampleBuffers,
    counters: KernelCounterReader,
    session_started: bool,
}

impl Recorder {
    pub fn open(path: &Path, retention: Retention) -> HistoryResult<Self> {
        Ok(Self {
            writer: HistoryWriter::spawn(path, retention)?,
            accumulator: Accumulator::new(),
            samples: SampleBuffers::default(),
            counters: KernelCounterReader::open(),
            session_started: false,
        })
    }

    pub fn record(&mut self, snapshot: &Snapshot) {
        if !self.session_started {
            self.session_started = true;
            let _ = self
                .writer
                .record_session_start(self.counters.read(snapshot));
        }
        let samples = self.samples.fill(snapshot);
        if let Some(batch) = self.accumulator.record(&samples) {
            self.writer.submit(batch);
            self.writer.checkpoint(self.counters.read(snapshot));
        }
    }

    pub fn save_alert(&self, alert: StoredAlert) {
        let _ = self.writer.save_alert(alert);
    }

    pub fn set_retention(&self, retention: Retention) {
        let _ = self.writer.set_retention(retention);
    }

    pub fn delete_all(&self) {
        let _ = self.writer.delete_all();
    }

    /// Writes the unfinished minute and the latest counters, then closes
    /// the database.
    pub fn finish(mut self, snapshot: &Snapshot) {
        if let Some(batch) = self.accumulator.flush() {
            self.writer.submit(batch);
        }
        if self.session_started {
            self.writer.checkpoint(self.counters.read(snapshot));
        }
        let _ = self.writer.shutdown();
    }
}
