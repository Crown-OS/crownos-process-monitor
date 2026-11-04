//! Paths through pixel points, where a NaN `y` breaks the outline.

use crownui::ext::Path;
use crownui::prelude::Point;

fn with_capacity(commands: usize) -> Path {
    Path {
        commands: Vec::with_capacity(commands),
    }
}

fn runs(points: &[Point]) -> impl Iterator<Item = &[Point]> {
    points
        .split(|point| point.y.is_nan())
        .filter(|run| !run.is_empty())
}

pub fn line_path(points: &[Point]) -> Path {
    runs(points).fold(with_capacity(points.len()), |path, run| {
        let (first, rest) = (run[0], &run[1..]);
        rest.iter()
            .fold(path.move_to(first), |path, point| path.line_to(*point))
    })
}

/// The region between the outline and the horizontal line at `baseline`.
pub fn area_path(points: &[Point], baseline: f32) -> Path {
    runs(points).fold(with_capacity(points.len() + 4), |path, run| {
        let (first, last) = (run[0], run[run.len() - 1]);
        run.iter()
            .fold(
                path.move_to(Point::new(first.x, baseline)),
                |path, point| path.line_to(*point),
            )
            .line_to(Point::new(last.x, baseline))
            .close()
    })
}

/// The region between two outlines of equal length, skipping columns where either is a gap.
pub fn band_path(upper: &[Point], lower: &[Point]) -> Path {
    let mut path = with_capacity(upper.len() * 2 + 2);
    let mut start = 0;
    let count = upper.len().min(lower.len());
    while start < count {
        let finite = |index: usize| !upper[index].y.is_nan() && !lower[index].y.is_nan();
        if !finite(start) {
            start += 1;
            continue;
        }
        let end = (start..count)
            .find(|index| !finite(*index))
            .unwrap_or(count);
        path = upper[start..end]
            .iter()
            .skip(1)
            .fold(path.move_to(upper[start]), |path, point| {
                path.line_to(*point)
            });
        path = lower[start..end]
            .iter()
            .rev()
            .fold(path, |path, point| path.line_to(*point))
            .close();
        start = end;
    }
    path
}

/// A horizontal line at `y` drawn as `dash`-long segments `gap` apart.
pub fn dashed_line(y: f32, width: f32, dash: f32, gap: f32) -> Path {
    let step = dash + gap;
    let count = (width / step).ceil().max(0.0) as usize;
    (0..count).fold(with_capacity(count * 2), |path, index| {
        let start = index as f32 * step;
        path.move_to(Point::new(start, y))
            .line_to(Point::new((start + dash).min(width), y))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_split_lines_into_separate_contours() {
        let points = [
            Point::new(0.0, 1.0),
            Point::new(1.0, 2.0),
            Point::new(2.0, f32::NAN),
            Point::new(3.0, 1.0),
            Point::new(4.0, 1.0),
        ];
        assert_eq!(line_path(&points).commands.len(), 4);
        assert_eq!(area_path(&points, 10.0).commands.len(), 2 * 5);
    }

    #[test]
    fn bands_walk_the_upper_edge_forward_and_the_lower_back() {
        let upper = [Point::new(0.0, 1.0), Point::new(1.0, 1.0)];
        let lower = [Point::new(0.0, 5.0), Point::new(1.0, 5.0)];
        let band = band_path(&upper, &lower);
        assert_eq!(band.commands.len(), 5);
        assert_eq!(band.bounds().size.height, 4.0);
    }

    #[test]
    fn dashes_cover_the_width() {
        let dashes = dashed_line(5.0, 20.0, 4.0, 4.0);
        assert_eq!(dashes.commands.len(), 6);
        assert_eq!(dashes.bounds().max_x(), 20.0);
    }
}
