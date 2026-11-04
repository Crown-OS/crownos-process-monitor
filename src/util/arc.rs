use std::f32::consts::{FRAC_PI_2, TAU};

use crownui::ext::Path;
use crownui::prelude::Point;

const MAX_SEGMENT_SWEEP: f32 = FRAC_PI_2;

/// A circular arc as cubic Béziers: starts at `start` radians (0 is 3 o'clock, clockwise on
/// screen) and sweeps `sweep` radians.
pub fn arc_path(center: Point, radius: f32, start: f32, sweep: f32) -> Path {
    let segments = (sweep.abs() / MAX_SEGMENT_SWEEP).ceil().max(1.0) as u32;
    let step = sweep / segments as f32;
    let handle = 4.0 / 3.0 * (step / 4.0).tan();
    let on_circle = |angle: f32| {
        Point::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        )
    };
    let tangent = |angle: f32| Point::new(-radius * angle.sin(), radius * angle.cos());
    let mut path = Path {
        commands: Vec::with_capacity(segments as usize + 1),
    }
    .move_to(on_circle(start));
    for index in 0..segments {
        let from = start + step * index as f32;
        let to = from + step;
        let (from_point, to_point) = (on_circle(from), on_circle(to));
        let (from_tangent, to_tangent) = (tangent(from), tangent(to));
        path = path.cubic_to(
            Point::new(
                from_point.x + handle * from_tangent.x,
                from_point.y + handle * from_tangent.y,
            ),
            Point::new(
                to_point.x - handle * to_tangent.x,
                to_point.y - handle * to_tangent.y,
            ),
            to_point,
        );
    }
    path
}

/// The angle of `point` around `center` in `0..TAU`, measured like [`arc_path`]: 0 at 3 o'clock,
/// growing clockwise on screen.
pub fn clockwise_angle(center: Point, point: Point) -> f32 {
    (point.y - center.y)
        .atan2(point.x - center.x)
        .rem_euclid(TAU)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::PI;

    use super::*;

    const TOLERANCE: f32 = 1e-3;

    #[test]
    fn a_quarter_turn_from_three_oclock_stays_in_its_quadrant() {
        let path = arc_path(Point::new(10.0, 10.0), 5.0, 0.0, FRAC_PI_2);
        let bounds = path.bounds();
        assert_eq!(path.commands.len(), 2);
        assert!((bounds.min_x() - 10.0).abs() < TOLERANCE);
        assert!((bounds.min_y() - 10.0).abs() < TOLERANCE);
        assert!((bounds.max_x() - 15.0).abs() < TOLERANCE);
        assert!((bounds.max_y() - 15.0).abs() < TOLERANCE);
    }

    #[test]
    fn long_sweeps_are_split_into_quarter_segments() {
        assert_eq!(arc_path(Point::ZERO, 1.0, 0.0, PI).commands.len(), 3);
        assert_eq!(arc_path(Point::ZERO, 1.0, 0.0, TAU).commands.len(), 5);
    }

    #[test]
    fn angles_grow_clockwise_from_three_oclock() {
        let center = Point::new(5.0, 5.0);
        assert!(clockwise_angle(center, Point::new(9.0, 5.0)).abs() < TOLERANCE);
        assert!((clockwise_angle(center, Point::new(5.0, 9.0)) - FRAC_PI_2).abs() < TOLERANCE);
        assert!((clockwise_angle(center, Point::new(1.0, 5.0)) - PI).abs() < TOLERANCE);
        assert!(
            (clockwise_angle(center, Point::new(5.0, 1.0)) - 3.0 * FRAC_PI_2).abs() < TOLERANCE
        );
    }
}
