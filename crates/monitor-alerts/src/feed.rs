use monitor_core::util::time::local_midnight;

use crate::alert::{Alert, AlertKind, AlertRecord};
use crate::quiet::QuietScope;

/// Oldest unpinned rows are dropped beyond this; history keeps the rest.
const MAX_ENTRIES: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(u64);

#[derive(Clone, Debug, PartialEq)]
pub struct FeedEntry {
    pub id: EntryId,
    pub record: AlertRecord,
}

/// One local calendar day of the feed, newest entry first.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedDay<'a> {
    pub day_start_unix: i64,
    pub entries: Vec<&'a FeedEntry>,
}

/// The Worth a Look feed: repeats of the same alert on the same local day
/// merge into one row with a count.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorthALook {
    entries: Vec<FeedEntry>,
    next_id: u64,
}

impl WorthALook {
    pub fn new() -> Self {
        Self::default()
    }

    /// Restores rows loaded from history.
    pub fn from_records(records: impl IntoIterator<Item = AlertRecord>) -> Self {
        let mut feed = Self::new();
        let mut records: Vec<_> = records.into_iter().collect();
        records.sort_by_key(|record| record.start_unix);
        for record in records {
            feed.push(record);
        }
        feed
    }

    pub fn entries(&self) -> &[FeedEntry] {
        &self.entries
    }

    /// Adds an alert, merging it into today's row for the same kind and app.
    /// Returns the row to persist.
    pub fn record(&mut self, alert: &Alert, utc_offset_seconds: i32) -> &AlertRecord {
        let day = local_midnight(alert.at_unix, utc_offset_seconds);
        let alert_app = alert.app.as_ref().map(|app| &app.key);
        let existing = self.entries.iter().rposition(|entry| {
            entry.record.kind == alert.kind
                && entry.record.app_key() == alert_app
                && local_midnight(entry.record.start_unix, utc_offset_seconds) == day
        });
        let index = match existing {
            Some(index) => {
                let record = &mut self.entries[index].record;
                record.count = record.count.saturating_add(1);
                record.end_unix = record.end_unix.max(alert.at_unix);
                record.peak = record.peak.max(alert.value);
                index
            }
            None => self.push(AlertRecord {
                kind: alert.kind,
                app: alert.app.clone(),
                start_unix: alert.at_unix,
                end_unix: alert.at_unix,
                peak: alert.value,
                limit: alert.limit,
                count: 1,
                quieted: false,
                pinned: false,
            }),
        };
        &self.entries[index].record
    }

    /// Rows grouped by local day, newest day first, optionally narrowed to a kind.
    pub fn days(&self, kind: Option<AlertKind>, utc_offset_seconds: i32) -> Vec<FeedDay<'_>> {
        let mut days: Vec<FeedDay<'_>> = Vec::new();
        let matching = self
            .entries
            .iter()
            .rev()
            .filter(|entry| kind.is_none_or(|kind| entry.record.kind == kind));
        for entry in matching {
            let day_start_unix = local_midnight(entry.record.start_unix, utc_offset_seconds);
            match days.last_mut() {
                Some(day) if day.day_start_unix == day_start_unix => day.entries.push(entry),
                _ => days.push(FeedDay {
                    day_start_unix,
                    entries: vec![entry],
                }),
            }
        }
        days
    }

    pub fn pinned(&self) -> impl Iterator<Item = &FeedEntry> {
        self.entries.iter().filter(|entry| entry.record.pinned)
    }

    /// Returns the changed row to persist.
    pub fn set_pinned(&mut self, id: EntryId, pinned: bool) -> Option<&AlertRecord> {
        let entry = self.entries.iter_mut().find(|entry| entry.id == id)?;
        entry.record.pinned = pinned;
        Some(&entry.record)
    }

    /// Marks rows covered by a new quiet rule; returns them to persist.
    pub fn mark_quieted<'a>(
        &'a mut self,
        scope: &'a QuietScope,
    ) -> impl Iterator<Item = &'a AlertRecord> {
        self.entries
            .iter_mut()
            .filter(move |entry| scope.matches(entry.record.app_key(), entry.record.kind))
            .map(|entry| {
                entry.record.quieted = true;
                &entry.record
            })
    }

    fn push(&mut self, record: AlertRecord) -> usize {
        if self.entries.len() >= MAX_ENTRIES
            && let Some(oldest) = self.entries.iter().position(|entry| !entry.record.pinned)
        {
            self.entries.remove(oldest);
        }
        self.entries.push(FeedEntry {
            id: EntryId(self.next_id),
            record,
        });
        self.next_id += 1;
        self.entries.len() - 1
    }
}
