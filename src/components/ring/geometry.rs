use std::f32::consts::{FRAC_PI_2, TAU};

/// Where segments start: 12 o'clock.
pub const RING_START: f32 = -FRAC_PI_2;

/// One segment's arc in radians, as `arc_path` takes it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentArc {
    pub start: f32,
    pub sweep: f32,
}

/// Lays `values` around the ring clockwise from [`RING_START`], `gap` radians apart. Every value
/// gets an arc (empty for zero, negative or NaN values) so indices match the caller's segments.
pub fn segment_arcs(values: &[f32], gap: f32, out: &mut Vec<SegmentArc>) {
    out.clear();
    let positive = |value: f32| value.is_finite() && value > 0.0;
    let (total, count) = values
        .iter()
        .filter(|value| positive(**value))
        .fold((0.0, 0_u32), |(total, count), value| {
            (total + value, count + 1)
        });
    let gap = if count > 1 { gap } else { 0.0 };
    let available = (TAU - gap * count as f32).max(0.0);
    let mut start = RING_START + gap / 2.0;
    out.extend(values.iter().map(|value| {
        let sweep = if positive(*value) && total > 0.0 {
            value / total * available
        } else {
            0.0
        };
        let arc = SegmentArc { start, sweep };
        if sweep > 0.0 {
            start += sweep + gap;
        }
        arc
    }));
}

/// The segment under `angle` (in `arc_path` radians), if any.
pub fn segment_at(arcs: &[SegmentArc], angle: f32) -> Option<usize> {
    arcs.iter()
        .position(|arc| arc.sweep > 0.0 && (angle - arc.start).rem_euclid(TAU) < arc.sweep)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f32 = 1e-4;

    #[test]
    fn segments_share_the_circle_minus_the_gaps() {
        let mut arcs = Vec::new();
        segment_arcs(&[1.0, 0.0, 3.0], 0.1, &mut arcs);
        assert_eq!(arcs.len(), 3);
        assert_eq!(arcs[1].sweep, 0.0);
        let swept: f32 = arcs.iter().map(|arc| arc.sweep).sum();
        assert!((swept - (TAU - 0.2)).abs() < TOLERANCE);
        assert!((arcs[2].sweep / arcs[0].sweep - 3.0).abs() < TOLERANCE);
        assert!((arcs[2].start - (arcs[0].start + arcs[0].sweep + 0.1)).abs() < TOLERANCE);
    }

    #[test]
    fn a_single_segment_closes_the_ring_without_a_gap() {
        let mut arcs = Vec::new();
        segment_arcs(&[5.0], 0.1, &mut arcs);
        assert!((arcs[0].sweep - TAU).abs() < TOLERANCE);
    }

    #[test]
    fn hit_testing_finds_the_segment_under_an_angle() {
        let mut arcs = Vec::new();
        segment_arcs(&[1.0, 1.0], 0.0, &mut arcs);
        assert_eq!(segment_at(&arcs, 0.0), Some(0));
        assert_eq!(segment_at(&arcs, std::f32::consts::PI), Some(1));
        segment_arcs(&[], 0.0, &mut arcs);
        assert_eq!(segment_at(&arcs, 0.0), None);
    }
}
