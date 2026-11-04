/// A sample kept for drawing: its position in the source slice and its value (NaN for a gap).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartPoint {
    pub index: f32,
    pub value: f32,
}

impl ChartPoint {
    const GAP: Self = Self {
        index: f32::NAN,
        value: f32::NAN,
    };

    pub fn is_gap(self) -> bool {
        self.value.is_nan()
    }
}

const POINTS_PER_COLUMN: usize = 4;
const POINTS_PER_PIXEL: usize = 2;

/// M4 decimation: keeps the first, lowest, highest and last sample of each column so the drawn
/// line looks exactly like the full series, with at most two points per pixel of `width`. NaN
/// samples become single gap markers that break the line.
pub fn decimate_m4(samples: &[f32], width: f32, out: &mut Vec<ChartPoint>) {
    out.clear();
    let budget = (width.max(1.0) as usize) * POINTS_PER_PIXEL;
    if samples.len() <= budget {
        for (index, value) in samples.iter().enumerate() {
            push_point(out, index, *value);
        }
        return;
    }
    let columns = (budget / POINTS_PER_COLUMN).max(1);
    let mut column = ColumnExtremes::default();
    let mut current_column = 0;
    for (index, value) in samples.iter().copied().enumerate() {
        let column_index = index * columns / samples.len();
        if column_index != current_column {
            column.flush(out);
            current_column = column_index;
        }
        if value.is_nan() {
            column.flush(out);
            push_gap(out);
        } else {
            column.add(index, value);
        }
    }
    column.flush(out);
}

/// The mean of each of `columns` equal slices of `samples` (NaN where a slice has no finite
/// sample); passes short series through unchanged.
pub fn column_means(samples: &[f32], columns: usize, out: &mut Vec<f32>) {
    out.clear();
    if samples.len() <= columns {
        out.extend_from_slice(samples);
        return;
    }
    out.extend((0..columns).map(|column| {
        let start = column * samples.len() / columns;
        let end = (column + 1) * samples.len() / columns;
        let (sum, count) = samples[start..end]
            .iter()
            .filter(|value| value.is_finite())
            .fold((0.0, 0_u32), |(sum, count), value| (sum + value, count + 1));
        if count == 0 {
            f32::NAN
        } else {
            sum / count as f32
        }
    }));
}

fn push_point(out: &mut Vec<ChartPoint>, index: usize, value: f32) {
    if value.is_nan() {
        push_gap(out);
    } else {
        out.push(ChartPoint {
            index: index as f32,
            value,
        });
    }
}

fn push_gap(out: &mut Vec<ChartPoint>) {
    if out.last().is_some_and(|last| !last.is_gap()) {
        out.push(ChartPoint::GAP);
    }
}

#[derive(Default)]
struct ColumnExtremes {
    first: Option<(usize, f32)>,
    lowest: (usize, f32),
    highest: (usize, f32),
    last: (usize, f32),
}

impl ColumnExtremes {
    fn add(&mut self, index: usize, value: f32) {
        let sample = (index, value);
        if self.first.is_none() {
            self.first = Some(sample);
            self.lowest = sample;
            self.highest = sample;
        } else if value < self.lowest.1 {
            self.lowest = sample;
        } else if value > self.highest.1 {
            self.highest = sample;
        }
        self.last = sample;
    }

    fn flush(&mut self, out: &mut Vec<ChartPoint>) {
        let Some(first) = self.first.take() else {
            return;
        };
        let mut kept = [first, self.lowest, self.highest, self.last];
        kept.sort_unstable_by_key(|(index, _)| *index);
        let mut previous = None;
        for (index, value) in kept {
            if previous != Some(index) {
                out.push(ChartPoint {
                    index: index as f32,
                    value,
                });
                previous = Some(index);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(points: &[ChartPoint]) -> impl Iterator<Item = f32> + '_ {
        points
            .iter()
            .filter(|point| !point.is_gap())
            .map(|point| point.value)
    }

    #[test]
    fn short_series_pass_through() {
        let mut out = Vec::new();
        decimate_m4(&[1.0, 2.0, 3.0], 100.0, &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(
            out[2],
            ChartPoint {
                index: 2.0,
                value: 3.0
            }
        );
    }

    #[test]
    fn long_series_keep_at_most_two_points_per_pixel_and_every_extreme() {
        let samples: Vec<f32> = (0..10_000)
            .map(|index| (index as f32 / 37.0).sin())
            .collect();
        let mut out = Vec::new();
        decimate_m4(&samples, 100.0, &mut out);
        assert!(out.len() <= 200, "{} points", out.len());
        let peak = values(&out).fold(f32::MIN, f32::max);
        let trough = values(&out).fold(f32::MAX, f32::min);
        assert_eq!(peak, samples.iter().copied().fold(f32::MIN, f32::max));
        assert_eq!(trough, samples.iter().copied().fold(f32::MAX, f32::min));
        assert_eq!(out.first().map(|point| point.index), Some(0.0));
        assert_eq!(out.last().map(|point| point.index), Some(9_999.0));
        assert!(out.windows(2).all(|pair| pair[0].index < pair[1].index));
    }

    #[test]
    fn nan_samples_become_one_gap_marker() {
        let mut samples = vec![1.0_f32; 4_000];
        samples[1_000..1_500].fill(f32::NAN);
        let mut out = Vec::new();
        decimate_m4(&samples, 50.0, &mut out);
        assert_eq!(out.iter().filter(|point| point.is_gap()).count(), 1);
        let mut short = Vec::new();
        decimate_m4(&[f32::NAN, 1.0, f32::NAN, f32::NAN, 2.0], 10.0, &mut short);
        assert_eq!(short.len(), 3);
        assert!(short[1].is_gap());
    }

    #[test]
    fn column_means_average_finite_samples() {
        let mut out = Vec::new();
        column_means(&[1.0, 3.0, f32::NAN, 4.0, f32::NAN, f32::NAN], 3, &mut out);
        assert_eq!(out[0], 2.0);
        assert_eq!(out[1], 4.0);
        assert!(out[2].is_nan());
    }
}
