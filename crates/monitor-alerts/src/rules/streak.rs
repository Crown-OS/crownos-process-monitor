use monitor_core::util::time::MINUTE_SECONDS;

/// Counts consecutive over-limit minutes; a missing or calm minute resets it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Streak {
    minutes: u32,
    last_minute_unix: Option<i64>,
}

impl Streak {
    pub fn update(&mut self, minute_unix: i64, over: bool) -> u32 {
        if !over {
            *self = Self::default();
            return 0;
        }
        let continues = self.last_minute_unix == Some(minute_unix - MINUTE_SECONDS);
        self.minutes = if continues {
            self.minutes.saturating_add(1)
        } else {
            1
        };
        self.last_minute_unix = Some(minute_unix);
        self.minutes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resets_on_calm_or_missing_minutes() {
        let mut streak = Streak::default();
        assert_eq!(streak.update(0, true), 1);
        assert_eq!(streak.update(60, true), 2);
        assert_eq!(streak.update(180, true), 1);
        assert_eq!(streak.update(240, false), 0);
        assert_eq!(streak.update(300, true), 1);
    }
}
