use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};

use super::SETTINGS_SECTION;
use super::insights::{INSIGHTS_SECTION, InsightSettings};
use super::model::MonitorSettings;

const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

enum WriterJob {
    Save(MonitorSettings),
    SaveInsights(Box<InsightSettings>),
    Flush(mpsc::SyncSender<()>),
}

/// Writes the settings section on its own thread, so a click never waits on the disk.
#[derive(Clone)]
pub struct SettingsWriter {
    jobs: Sender<WriterJob>,
}

impl SettingsWriter {
    pub fn spawn() -> Result<Self> {
        let (jobs, queue) = mpsc::channel();
        thread::Builder::new()
            .name("monitor-settings".into())
            .spawn(move || write_queued(&queue))
            .context("Failed to start the settings writer thread")?;
        Ok(Self { jobs })
    }

    pub fn save(&self, settings: MonitorSettings) {
        if self.jobs.send(WriterJob::Save(settings)).is_err() {
            eprintln!("process-monitor: the settings writer stopped; a change was not saved");
        }
    }

    pub fn save_insights(&self, insights: InsightSettings) {
        if self
            .jobs
            .send(WriterJob::SaveInsights(Box::new(insights)))
            .is_err()
        {
            eprintln!("process-monitor: the settings writer stopped; a change was not saved");
        }
    }

    /// Waits until every queued write is on disk.
    pub fn flush(&self) {
        let (done, wait) = mpsc::sync_channel(0);
        if self.jobs.send(WriterJob::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(FLUSH_TIMEOUT);
        }
    }
}

/// Reads the insights section; blocks on the disk.
pub fn load_insights() -> InsightSettings {
    crownconfig::load_section(INSIGHTS_SECTION)
}

fn write_queued(queue: &Receiver<WriterJob>) {
    while let Ok(job) = queue.recv() {
        match job {
            WriterJob::Save(settings) => {
                if let Err(error) = crownconfig::save_section(SETTINGS_SECTION, &settings) {
                    eprintln!("process-monitor: {error}");
                }
            }
            WriterJob::SaveInsights(insights) => {
                if let Err(error) = crownconfig::save_section(INSIGHTS_SECTION, &*insights) {
                    eprintln!("process-monitor: {error}");
                }
            }
            WriterJob::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}
