use std::time::Duration;

/// Whether `interval` of monotonic time passed since `last`; always true
/// before the first run.
#[must_use]
pub fn interval_elapsed(last: Option<Duration>, now: Duration, interval: Duration) -> bool {
    last.is_none_or(|last| now.saturating_sub(last) >= interval)
}

/// Gates work that should run at most once per interval of monotonic time.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cadence {
    last: Option<Duration>,
}

impl Cadence {
    /// Whether the work is due at `now`; when it is, `now` counts as its run.
    #[must_use]
    pub fn due(&mut self, now: Duration, interval: Duration) -> bool {
        if !interval_elapsed(self.last, now, interval) {
            return false;
        }
        self.last = Some(now);
        true
    }

    /// Makes the next [`Self::due`] fire; returns whether it had run since
    /// the last reset.
    pub fn reset(&mut self) -> bool {
        self.last.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_fire_on_first_and_due_ticks() {
        let now = Duration::from_secs(100);
        let interval = Duration::from_secs(10);
        assert!(interval_elapsed(None, now, interval));
        assert!(!interval_elapsed(
            Some(Duration::from_secs(95)),
            now,
            interval
        ));
        assert!(interval_elapsed(
            Some(Duration::from_secs(90)),
            now,
            interval
        ));
    }

    #[test]
    fn runs_once_per_interval() {
        let mut cadence = Cadence::default();
        let interval = Duration::from_secs(2);
        assert!(cadence.due(Duration::from_secs(10), interval));
        assert!(!cadence.due(Duration::from_secs(11), interval));
        assert!(cadence.due(Duration::from_secs(12), interval));
        cadence.reset();
        assert!(cadence.due(Duration::from_secs(12), interval));
    }
}
