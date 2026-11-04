use monitor_core::util::time::MINUTE_SECONDS;

const SEGMENTS: usize = 6;
/// A later segment may dip this far below the previous one and still count
/// as growing (allocators release memory in steps).
const DIP_TOLERANCE: f64 = 0.02;

/// Detects steady memory growth over a window from per-minute averages.
/// The window is split into segments and each keeps only its minimum, so
/// spikes are ignored and memory stays constant per app.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrowthWindow {
    segment_minima: [f64; SEGMENTS],
    filled_segments: usize,
    current_minimum: f64,
    current_minutes: u32,
    last_minute_unix: Option<i64>,
}

impl Default for GrowthWindow {
    fn default() -> Self {
        Self {
            segment_minima: [0.0; SEGMENTS],
            filled_segments: 0,
            current_minimum: f64::INFINITY,
            current_minutes: 0,
            last_minute_unix: None,
        }
    }
}

impl GrowthWindow {
    /// Returns the growth in bytes each time a segment completes a full,
    /// non-decreasing window.
    pub fn update(
        &mut self,
        minute_unix: i64,
        memory_bytes: f64,
        window_minutes: u32,
    ) -> Option<f64> {
        if self
            .last_minute_unix
            .is_some_and(|last| last != minute_unix - MINUTE_SECONDS)
        {
            *self = Self::default();
        }
        self.last_minute_unix = Some(minute_unix);
        self.current_minimum = self.current_minimum.min(memory_bytes);
        self.current_minutes += 1;
        let segment_minutes = (window_minutes / SEGMENTS as u32).max(1);
        if self.current_minutes < segment_minutes {
            return None;
        }
        if self.filled_segments == SEGMENTS {
            self.segment_minima.copy_within(1.., 0);
        } else {
            self.filled_segments += 1;
        }
        self.segment_minima[self.filled_segments - 1] = self.current_minimum;
        self.current_minimum = f64::INFINITY;
        self.current_minutes = 0;
        if self.filled_segments < SEGMENTS {
            return None;
        }
        let steady = self
            .segment_minima
            .windows(2)
            .all(|pair| pair[1] >= pair[0] * (1.0 - DIP_TOLERANCE));
        steady.then(|| self.segment_minima[SEGMENTS - 1] - self.segment_minima[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: f64 = 1_048_576.0;

    #[test]
    fn reports_steady_growth_ignoring_spikes() {
        let mut window = GrowthWindow::default();
        let growth: Vec<_> = (0..60)
            .filter_map(|minute| {
                let spike = if minute % 7 == 3 { 900.0 * MIB } else { 0.0 };
                let memory = 1000.0 * MIB + f64::from(minute) * 20.0 * MIB + spike;
                window.update(i64::from(minute) * 60, memory, 60)
            })
            .collect();
        assert_eq!(growth.len(), 1);
        assert!((growth[0] - 1000.0 * MIB).abs() < 1.0);
    }

    #[test]
    fn rejects_sawtooth() {
        let mut window = GrowthWindow::default();
        let growth = (0..120).find_map(|minute| {
            let memory = if (minute / 10) % 2 == 0 {
                3000.0
            } else {
                1000.0
            } * MIB;
            window.update(i64::from(minute) * 60, memory, 60)
        });
        assert_eq!(growth, None);
    }
}
