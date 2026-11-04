use crownui::ext::{DrawList, NodeId};
use crownui::kit::{Theme, use_theme};
use crownui::prelude::{Color, Cx, Memo, Runtime, Signal, Size, Styled, View, canvas, zstack};

use super::Readout;
use super::axis::AxisLabels;
use super::decimate::decimate_m4;
use super::hover::{crosshair, readout_label};
use super::paint::{fade_fill, fill, stroke_hairline, stroke_line};
use super::scale::{Plot, YRange, sample_at};
use super::source::SeriesSource;
use super::trace::{area_path, dashed_line, line_path};
use crate::components::pointer::pointer_tracker;

const DEFAULT_HEIGHT: f32 = 160.0;
const DASH_LENGTH: f32 = 4.0;
const DASH_GAP: f32 = 4.0;

/// A gradient-filled area under a rounded line. `source` lends the samples (oldest first, NaN
/// for a gap) to the sink it is given; the chart repaints when what it reads changes.
pub struct AreaChart<S> {
    source: S,
    color: Memo<Color>,
    range: YRange,
    reference: Option<f32>,
    readout: Option<Readout>,
    labels: Option<AxisLabels>,
    height: f32,
}

pub fn area_chart<S>(source: S, color: Memo<Color>) -> AreaChart<S>
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + Clone + 'static,
{
    AreaChart {
        source,
        color,
        range: YRange::AUTO,
        reference: None,
        readout: None,
        labels: None,
        height: DEFAULT_HEIGHT,
    }
}

impl<S> AreaChart<S>
where
    S: SeriesSource,
{
    #[must_use]
    pub fn range(mut self, range: YRange) -> Self {
        self.range = range;
        self
    }

    /// A dashed horizontal line at `value`, such as a 90 % threshold or Tjmax.
    #[must_use]
    pub fn reference(mut self, value: f32) -> Self {
        self.reference = Some(value);
        self
    }

    /// Shows a crosshair and the value under the pointer, formatted by `readout`.
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

impl<S> View for AreaChart<S>
where
    S: SeriesSource,
{
    fn build(self, cx: &mut Cx) -> NodeId {
        let theme = use_theme(cx);
        let hover = cx.signal(None);
        let surface = canvas(painter(
            self.source.clone(),
            self.color,
            self.range,
            self.reference,
            theme,
        ))
        .absolute()
        .inset(0.0);
        let source = self.source;
        let interaction = self.readout.map(|readout| {
            zstack((
                crosshair(theme, hover),
                readout_label(hover, move |runtime, hover| {
                    let mut value = f32::NAN;
                    source(runtime, &mut |samples| {
                        value = sample_at(samples, hover.fraction_x());
                    });
                    readout(runtime, value)
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

fn painter<S>(
    source: S,
    color: Memo<Color>,
    range: YRange,
    reference: Option<f32>,
    theme: Signal<Theme>,
) -> impl FnMut(&mut Runtime, Size, &mut DrawList) + 'static
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + 'static,
{
    let mut decimated = Vec::new();
    let mut pixels = Vec::new();
    move |runtime, size, list| {
        let mut samples = 0;
        source(runtime, &mut |series| {
            samples = series.len();
            decimate_m4(series, size.width, &mut decimated);
        });
        let (min, max) = range.resolve(decimated.iter().map(|point| point.value));
        let plot = Plot {
            size,
            samples,
            min,
            max,
        };
        pixels.clear();
        pixels.extend(decimated.iter().map(|point| plot.point(*point)));
        let color = color.get(runtime);
        fill(
            list,
            area_path(&pixels, size.height),
            fade_fill(color, 0.0, size.height),
        );
        stroke_line(list, line_path(&pixels), color);
        if let Some(value) = reference.filter(|value| (min..=max).contains(value)) {
            let muted = theme.with(runtime, |theme| theme.text.muted);
            stroke_hairline(
                list,
                dashed_line(plot.y(value), size.width, DASH_LENGTH, DASH_GAP),
                muted,
            );
        }
    }
}
