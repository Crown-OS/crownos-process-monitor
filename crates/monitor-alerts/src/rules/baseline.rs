/// Weight of the newest minute in the moving average: roughly the last
/// half hour shapes an app's usual rate.
const SMOOTHING: f64 = 1.0 / 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BurstRule {
    pub factor: f64,
    pub floor: f64,
    pub warmup_minutes: u32,
}

/// An exponentially weighted moving average of a per-minute rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Baseline {
    average: f64,
    minutes: u32,
}

impl Baseline {
    /// Whether `rate` is a burst against the baseline so far; then folds it in.
    pub fn check_and_update(&mut self, rate: f64, rule: &BurstRule) -> bool {
        let burst = self.minutes >= rule.warmup_minutes
            && rate >= rule.floor
            && rate >= self.average * rule.factor;
        self.average += SMOOTHING * (rate - self.average);
        self.minutes = self.minutes.saturating_add(1);
        burst
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_warmup_floor_and_factor() {
        let rule = BurstRule {
            factor: 10.0,
            floor: 100.0,
            warmup_minutes: 3,
        };
        let mut baseline = Baseline::default();
        assert!(!baseline.check_and_update(1_000.0, &rule));
        let mut calm = Baseline::default();
        assert!((0..3).all(|_| !calm.check_and_update(5.0, &rule)));
        assert!(!calm.check_and_update(60.0, &rule));
        assert!(calm.check_and_update(500.0, &rule));
    }
}
