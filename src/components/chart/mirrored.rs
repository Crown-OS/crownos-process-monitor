use crownui::ext::{DrawList, NodeId};
use crownui::kit::{Theme, use_theme};
use crownui::prelude::{
    CornerRadii, Cx, Point, Rect, Runtime, Signal, Size, Styled, View, canvas, zstack,
};

use super::Readout;
use super::axis::AxisLabels;
use super::decimate::{ChartPoint, decimate_m4};
use super::hover::{crosshair, readout_label};
use super::lines::Series;
use super::paint::{fade_fill, fill, stroke_line};
use super::scale::{YRange, sample_at};
use super::source::PairSource;
use super::trace::{area_path, line_path};
use crate::components::pointer::pointer_tracker;

const DEFAULT_HEIGHT: f32 = 180.0;

/// Two series mirrored around a middle axis: the first above it (download, read), the second
/// below (upload, write), on one shared scale.
pub struct MirroredChart<S> {
    source: S,
    series: [Series; 2],
    range: YRange,
    readout: Option<Readout>,
    labels: Option<AxisLabels>,
    height: f32,
}

pub fn mirrored_chart<S>(source: S, series: [Series; 2]) -> MirroredChart<S>
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32], &[f32])) + Clone + 'static,
{
    MirroredChart {
        source,
        series,
        range: YRange::AUTO,
        readout: None,
        labels: None,
        height: DEFAULT_HEIGHT,
    }
}

impl<S> MirroredChart<S>
where
    S: PairSource,
{
    #[must_use]
    pub fn range(mut self, range: YRange) -> Self {
        self.range = range;
        self
    }

    /// Shows a crosshair and both values under the pointer: `readout` formats one of them.
    #[must_use]
    pub fn readout(mut self, readout: impl Fn(&mut Runtime, f32) -> String + 'static) -> Self {
        self.readout = Some(Box::new(readout));
        self
    }

    #[must_use]
    pub fn labels(mut self, labels: AxisLabels) -> Self {
        self.labels = Some(labels);
        self
    }

    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}

impl<S> View for MirroredChart<S>
where
    S: PairSource,
{
    fn build(self, cx: &mut Cx) -> NodeId {
        let theme = use_theme(cx);
        let hover = cx.signal(None);
        let surface = canvas(painter(self.source.clone(), self.series, self.range, theme))
            .absolute()
            .inset(0.0);
        let source = self.source;
        let [upper, lower] = self.series;
        let interaction = self.readout.map(|readout| {
            zstack((
                crosshair(theme, hover),
                readout_label(hover, move |runtime, hover| {
                    let (mut above, mut below) = (f32::NAN, f32::NAN);
                    source(runtime, &mut |first, second| {
                        above = sample_at(first, hover.fraction_x());
                        below = sample_at(second, hover.fraction_x());
                    });
                    format!(
                        "{} {}  {} {}",
                        upper.label,
                        readout(runtime, above),
                        lower.label,
                        readout(runtime, below)
                    )
                }),
            ))
            .absolute()
            .inset(0.0)
        });
        let labels = self.labels.map(AxisLabels::overlay);
        let chart = pointer_tracker(
            hover,
            zstack((surface, labels, interaction))
                .w_full()
                .h(self.height),
        )
        .w_full()
        .h(self.height);
        cx.build(chart)
    }
}

#[derive(Default)]
struct MirrorBuffers {
    above: Vec<ChartPoint>,
    below: Vec<ChartPoint>,
    pixels: Vec<Point>,
}

fn painter<S>(
    source: S,
    series: [Series; 2],
    range: YRange,
    theme: Signal<Theme>,
) -> impl FnMut(&mut Runtime, Size, &mut DrawList) + 'static
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32], &[f32])) + 'static,
{
    let mut buffers = MirrorBuffers::default();
    move |runtime, size, list| {
        let MirrorBuffers {
            above,
            below,
            pixels,
        } = &mut buffers;
        let mut samples = 0;
        source(runtime, &mut |first, second| {
            samples = first.len().max(second.len());
            decimate_m4(first, size.width, above);
            decimate_m4(second, size.width, below);
        });
        let (_, max) = range.resolve(above.iter().chain(below.iter()).map(|point| point.value));
        let axis = size.height / 2.0;
        let span = max.max(f32::EPSILON);
        let x = |index: f32| match samples {
            0 | 1 => size.width,
            samples => index / (samples - 1) as f32 * size.width,
        };
        let hairline = theme.with(runtime, |theme| theme.surface.hairline);
        list.fill(
            Rect::from_xywh(0.0, axis - 0.5, size.width, 1.0),
            CornerRadii::ZERO,
            hairline,
        );
        for (points, direction, entry) in [(&*above, -1.0, series[0]), (&*below, 1.0, series[1])] {
            pixels.clear();
            pixels.extend(points.iter().map(|point| {
                let offset = (point.value / span).clamp(0.0, 1.0) * axis;
                Point::new(x(point.index), axis + direction * offset)
            }));
            let color = entry.color.get(runtime);
            let edge = axis + direction * axis;
            fill(list, area_path(pixels, axis), fade_fill(color, edge, axis));
            stroke_line(list, line_path(pixels), color);
        }
    }
}
