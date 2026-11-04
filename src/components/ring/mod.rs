//! A donut ring of N segments with a centre label.

mod geometry;

use crownui::ext::{DrawCommand, DrawList, NodeId, Paint, Prop, Stroke};
use crownui::kit::use_theme;
use crownui::prelude::{
    Color, Cx, IntoProp, Point, Runtime, Signal, Size, Styled, View, bind, canvas, text, vstack,
    zstack,
};

use crate::components::pointer::{Hover, pointer_tracker};
use crate::util::arc::{arc_path, clockwise_angle};
use geometry::{SegmentArc, segment_arcs, segment_at};

const DEFAULT_DIAMETER: f32 = 140.0;
const THICKNESS_RATIO: f32 = 0.13;
const HOVER_GROWTH: f32 = 4.0;
const SEGMENT_GAP: f32 = 0.05;
const DIMMED_ALPHA: f32 = 0.45;
const LABEL_SIZE: f32 = 20.0;
const SUBLABEL_SIZE: f32 = 11.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingSegment {
    pub value: f32,
    pub color: Color,
}

/// A donut whose `source` lends the segments; the segment under the pointer is raised and its
/// index written to the caller's `hovered`.
pub struct Ring<S> {
    source: S,
    hovered: Signal<Option<usize>>,
    label: Prop<String>,
    sublabel: Prop<String>,
    diameter: f32,
}

pub fn ring<S>(source: S, hovered: Signal<Option<usize>>) -> Ring<S>
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[RingSegment])) + Clone + 'static,
{
    Ring {
        source,
        hovered,
        label: Prop::Static(String::new()),
        sublabel: Prop::Static(String::new()),
        diameter: DEFAULT_DIAMETER,
    }
}

impl<S> Ring<S>
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[RingSegment])) + Clone + 'static,
{
    #[must_use]
    pub fn label(mut self, label: impl IntoProp<String>) -> Self {
        self.label = label.into_prop();
        self
    }

    #[must_use]
    pub fn sublabel(mut self, sublabel: impl IntoProp<String>) -> Self {
        self.sublabel = sublabel.into_prop();
        self
    }

    #[must_use]
    pub fn diameter(mut self, diameter: f32) -> Self {
        self.diameter = diameter;
        self
    }
}

#[derive(Debug, Clone, Copy)]
struct RingShape {
    center: Point,
    radius: f32,
    thickness: f32,
}

impl RingShape {
    fn new(size: Size) -> Self {
        let diameter = size.width.min(size.height);
        let thickness = diameter * THICKNESS_RATIO;
        Self {
            center: Point::new(size.width / 2.0, size.height / 2.0),
            radius: (diameter - thickness - HOVER_GROWTH) / 2.0,
            thickness,
        }
    }

    fn segment_under(self, arcs: &[SegmentArc], point: Point) -> Option<usize> {
        let distance = (point.x - self.center.x).hypot(point.y - self.center.y);
        let reach = self.thickness / 2.0 + HOVER_GROWTH;
        if (distance - self.radius).abs() > reach {
            return None;
        }
        segment_at(arcs, clockwise_angle(self.center, point))
    }
}

fn segment_values<'a>(segments: &[RingSegment], values: &'a mut Vec<f32>) -> &'a [f32] {
    values.clear();
    values.extend(segments.iter().map(|segment| segment.value));
    values
}

impl<S> View for Ring<S>
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[RingSegment])) + Clone + 'static,
{
    fn build(self, cx: &mut Cx) -> NodeId {
        let theme = use_theme(cx);
        let hover = cx.signal(None::<Hover>);
        let hovered = self.hovered;
        let tracked = self.source.clone();
        let mut arcs = Vec::new();
        let mut values = Vec::new();
        cx.effect(move |runtime| {
            let index = hover.get(runtime).and_then(|hover| {
                tracked(runtime, &mut |segments| {
                    segment_arcs(
                        segment_values(segments, &mut values),
                        SEGMENT_GAP,
                        &mut arcs,
                    );
                });
                RingShape::new(hover.size).segment_under(&arcs, hover.position)
            });
            if hovered.with_untracked(runtime, |current| *current != index) {
                hovered.set(runtime, index);
            }
        });
        let surface = canvas(painter(self.source, hovered)).absolute().inset(0.0);
        let muted = bind(move |runtime| theme.with(runtime, |theme| theme.text.muted));
        let centre = vstack((
            text(self.label).text_size(LABEL_SIZE).font_semibold(),
            text(self.sublabel)
                .text_size(SUBLABEL_SIZE)
                .text_color(muted),
        ))
        .absolute()
        .inset(0.0)
        .items_center()
        .justify_center()
        .pointer_events_none();
        cx.build(
            pointer_tracker(hover, zstack((surface, centre)).size(self.diameter))
                .size(self.diameter),
        )
    }
}

fn painter<S>(
    source: S,
    hovered: Signal<Option<usize>>,
) -> impl FnMut(&mut Runtime, Size, &mut DrawList) + 'static
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[RingSegment])) + 'static,
{
    let mut arcs = Vec::new();
    let mut values = Vec::new();
    move |runtime, size, list| {
        let highlighted = hovered.get(runtime);
        let shape = RingShape::new(size);
        source(runtime, &mut |segments| {
            segment_arcs(
                segment_values(segments, &mut values),
                SEGMENT_GAP,
                &mut arcs,
            );
            for (index, (arc, segment)) in arcs.iter().zip(segments).enumerate() {
                if arc.sweep <= 0.0 {
                    continue;
                }
                let (width, alpha) = match highlighted {
                    Some(picked) if picked == index => (shape.thickness + HOVER_GROWTH, 1.0),
                    Some(_) => (shape.thickness, DIMMED_ALPHA),
                    None => (shape.thickness, 1.0),
                };
                list.push(DrawCommand::StrokePath(
                    arc_path(shape.center, shape.radius, arc.start, arc.sweep),
                    Stroke::new(width),
                    Paint::solid(segment.color.with_alpha_factor(alpha)),
                ));
            }
        });
    }
}
