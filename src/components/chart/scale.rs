use crownui::prelude::{Point, Size};

use super::decimate::ChartPoint;

const AUTO_HEADROOM: f32 = 0.15;
const EMPTY_AUTO_MAX: f32 = 1.0;

/// The value range a chart's height spans.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum YRange {
    Fixed {
        min: f32,
        max: f32,
    },
    /// From zero to the highest visible value plus `headroom` (a fraction of it).
    Auto {
        headroom: f32,
    },
}

impl YRange {
    pub const PERCENT: Self = Self::Fixed {
        min: 0.0,
        max: 100.0,
    };
    pub const AUTO: Self = Self::Auto {
        headroom: AUTO_HEADROOM,
    };

    pub fn resolve(self, values: impl Iterator<Item = f32>) -> (f32, f32) {
        match self {
            Self::Fixed { min, max } => (min, max),
            Self::Auto { headroom } => {
                let peak = values.filter(|value| value.is_finite()).fold(0.0, f32::max);
                if peak > 0.0 {
                    (0.0, peak * (1.0 + headroom))
                } else {
                    (0.0, EMPTY_AUTO_MAX)
                }
            }
        }
    }
}

/// Maps sample positions and values into a chart's pixels.
#[derive(Debug, Clone, Copy)]
pub struct Plot {
    pub size: Size,
    pub samples: usize,
    pub min: f32,
    pub max: f32,
}

impl Plot {
    pub fn x(&self, index: f32) -> f32 {
        match self.samples {
            0 | 1 => self.size.width,
            samples => index / (samples - 1) as f32 * self.size.width,
        }
    }

    pub fn y(&self, value: f32) -> f32 {
        let span = (self.max - self.min).max(f32::EPSILON);
        let fraction = ((value - self.min) / span).clamp(0.0, 1.0);
        if value.is_nan() {
            f32::NAN
        } else {
            self.size.height - fraction * self.size.height
        }
    }

    pub fn point(&self, point: ChartPoint) -> Point {
        Point::new(self.x(point.index), self.y(point.value))
    }
}

/// The sample nearest to `fraction` of the way along `samples`.
pub fn sample_at(samples: &[f32], fraction: f32) -> f32 {
    let Some(last) = samples.len().checked_sub(1) else {
        return f32::NAN;
    };
    let index = (fraction.clamp(0.0, 1.0) * last as f32).round() as usize;
    samples.get(index).copied().unwrap_or(f32::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_ranges_add_headroom_above_the_peak() {
        let (min, max) = YRange::AUTO.resolve([1.0, f32::NAN, 4.0].into_iter());
        assert_eq!(min, 0.0);
        assert!((max - 4.6).abs() < 1e-4);
        assert_eq!(YRange::AUTO.resolve(std::iter::empty()), (0.0, 1.0));
    }

    #[test]
    fn plots_flip_values_upward_and_keep_gaps() {
        let plot = Plot {
            size: Size::new(100.0, 50.0),
            samples: 11,
            min: 0.0,
            max: 100.0,
        };
        assert_eq!(plot.x(10.0), 100.0);
        assert_eq!(plot.y(100.0), 0.0);
        assert_eq!(plot.y(0.0), 50.0);
        assert!(plot.y(f32::NAN).is_nan());
    }

    #[test]
    fn the_nearest_sample_is_picked() {
        assert_eq!(sample_at(&[1.0, 2.0, 3.0], 0.9), 3.0);
        assert!(sample_at(&[], 0.5).is_nan());
    }
}
