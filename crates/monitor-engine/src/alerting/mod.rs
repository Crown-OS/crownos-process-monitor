mod observations;
mod quiet_file;
mod records;

use std::sync::Arc;

use monitor_alerts::{
    Alert, AlertDetector, AlertRecord, AppObservation, CustomLimit, EntryId, QuietScope,
    QuietUntil, Thresholds, WorthALook,
};
use monitor_history::StoredAlert;

pub(crate) use self::quiet_file::QuietFile;
pub(crate) use self::records::{from_stored, to_stored};
use crate::notify::Notifier;
use crate::util::local_time::utc_offset_seconds;
use crate::{EngineSettings, Notifications, Snapshot};

/// Runs the alert rules every tick and keeps the Worth a Look feed. The
/// feed is shared with published snapshots and copied only when it changes
/// while the UI still holds the previous one.
pub(crate) struct Alerting {
    detector: AlertDetector,
    feed: Arc<WorthALook>,
    observations: Vec<AppObservation>,
    fired: Vec<Alert>,
    notifier: Option<Notifier>,
    quiet_file: Option<QuietFile>,
}

impl Alerting {
    pub fn new(
        settings: &EngineSettings,
        past: impl IntoIterator<Item = AlertRecord>,
        notifications: Notifications,
        quiet_file: Option<QuietFile>,
    ) -> Self {
        let mut detector = AlertDetector::new(settings.thresholds.clone());
        detector.set_custom_limits(settings.custom_limits.iter().cloned());
        for rule in quiet_file.iter().flat_map(QuietFile::load) {
            detector.quiet(rule.scope, rule.until);
        }
        Self {
            detector,
            feed: Arc::new(WorthALook::from_records(past)),
            observations: Vec::new(),
            fired: Vec::new(),
            notifier: notifier_for(notifications),
            quiet_file,
        }
    }

    /// Spawns the notification thread when popups are turned on; dropping
    /// the notifier ends it.
    pub fn set_notifications(&mut self, notifications: Notifications) {
        if self.notifier.is_some() != (notifications == Notifications::Desktop) {
            self.notifier = notifier_for(notifications);
        }
    }

    fn save_quiet_rules(&self) {
        if let Some(file) = &self.quiet_file {
            file.save(self.detector.quiet_rules());
        }
    }

    pub fn configure(&mut self, thresholds: &Thresholds, custom_limits: &[CustomLimit]) {
        if self.detector.thresholds() != thresholds {
            self.detector.set_thresholds(thresholds.clone());
        }
        self.detector
            .set_custom_limits(custom_limits.iter().cloned());
    }

    /// Feeds one tick; every new alert is merged into the feed, handed to
    /// `persist` and shown as a desktop notification.
    pub fn observe(&mut self, snapshot: &Snapshot, mut persist: impl FnMut(StoredAlert)) {
        observations::fill(&mut self.observations, snapshot);
        let input = observations::input(snapshot, &self.observations);
        self.detector.observe(&input, &mut self.fired);
        if self.fired.is_empty() {
            return;
        }
        let offset = utc_offset_seconds();
        let feed = Arc::make_mut(&mut self.feed);
        for alert in self.fired.drain(..) {
            persist(to_stored(feed.record(&alert, offset)));
            if let Some(notifier) = &self.notifier {
                notifier.alert(&alert);
            }
        }
    }

    pub fn quiet(
        &mut self,
        scope: QuietScope,
        until: QuietUntil,
        persist: impl FnMut(StoredAlert),
    ) {
        Arc::make_mut(&mut self.feed)
            .mark_quieted(&scope)
            .map(to_stored)
            .for_each(persist);
        self.detector.quiet(scope, until);
        self.save_quiet_rules();
    }

    pub fn unquiet(&mut self, scope: &QuietScope) {
        self.detector.unquiet(scope);
        self.save_quiet_rules();
    }

    pub fn pin(&mut self, entry: EntryId, pinned: bool, mut persist: impl FnMut(StoredAlert)) {
        if let Some(record) = Arc::make_mut(&mut self.feed).set_pinned(entry, pinned) {
            persist(to_stored(record));
        }
    }

    pub fn clear(&mut self) {
        self.feed = Arc::default();
    }

    pub fn publish_into(&self, snapshot: &mut Snapshot) {
        if !Arc::ptr_eq(&snapshot.alerts, &self.feed) {
            snapshot.alerts = Arc::clone(&self.feed);
        }
        snapshot.strain = *self.detector.strain();
        let rules = self.detector.quiet_rules();
        if *snapshot.quiet_rules != *rules {
            snapshot.quiet_rules = rules.into();
        }
    }
}

fn notifier_for(notifications: Notifications) -> Option<Notifier> {
    match notifications {
        Notifications::Desktop => Notifier::spawn(),
        Notifications::Off => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_toggle_at_runtime() {
        let mut alerting = Alerting::new(&EngineSettings::default(), [], Notifications::Off, None);
        assert!(alerting.notifier.is_none());
        alerting.set_notifications(Notifications::Desktop);
        assert!(alerting.notifier.is_some());
        alerting.set_notifications(Notifications::Off);
        assert!(alerting.notifier.is_none());
    }
}
