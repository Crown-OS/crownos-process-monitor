use crownui::ext::{DrawList, NodeId};
use crownui::kit::tag_chip;
use crownui::prelude::{
    Color, Cx, Listeners, Memo, Role, Runtime, Semantic, Signal, Size, Styled, View, bind, canvas,
    hstack, vstack,
};

use super::decimate::{ChartPoint, decimate_m4};
use super::paint::stroke_line;
use super::scale::{Plot, YRange};
use super::source::MultiSeriesSource;
use super::trace::line_path;

const DEFAULT_HEIGHT: f32 = 160.0;
const LEGEND_GAP: f32 = 6.0;
const HIDDEN_OPACITY: f32 = 0.4;

/// One line of a multi-series chart and its legend entry.
#[derive(Clone, Copy)]
pub struct Series {
    pub label: &'static str,
    pub color: Memo<Color>,
}

/// Whether clicking a legend entry hides its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Legend {
    #[default]
    Toggles,
    Labels,
}

/// Several lines over one shared scale, with a legend below it. `source` lends one slice per
/// series, in the order of `series`.
pub struct LineChart<S> {
    source: S,
    series: Vec<Series>,
    range: YRange,
    height: f32,
    legend: Legend,
}

pub fn line_chart<S>(source: S, series: Vec<Series>) -> LineChart<S>
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[&[f32]])) + Clone + 'static,
{
    LineChart {
        source,
        series,
        range: YRange::AUTO,
        height: DEFAULT_HEIGHT,
        legend: Legend::Toggles,
    }
}

impl<S> LineChart<S>
where
    S: MultiSeriesSource,
{
    #[must_use]
    pub fn range(mut self, range: YRange) -> Self {
        self.range = range;
        self
    }

    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    #[must_use]
    pub fn legend(mut self, legend: Legend) -> Self {
        self.legend = legend;
        self
    }
}

impl<S> View for LineChart<S>
where
    S: MultiSeriesSource,
{
    fn build(self, cx: &mut Cx) -> NodeId {
        let hidden = cx.signal(0_u64);
        let legend: Vec<_> = self
            .series
            .iter()
            .enumerate()
            .map(|(index, series)| match self.legend {
                Legend::Toggles => legend_entry(*series, index, hidden).boxed(),
                Legend::Labels => tag_chip(series.label, series.color).boxed(),
            })
            .collect();
        let surface = canvas(painter(self.source, self.series, self.range, hidden))
            .w_full()
            .h(self.height);
        cx.build(
            vstack((surface, hstack(legend).gap(LEGEND_GAP)))
                .gap(LEGEND_GAP)
                .w_full(),
        )
    }
}

fn is_hidden(mask: u64, index: usize) -> bool {
    mask & (1 << index) != 0
}

fn legend_entry(series: Series, index: usize, hidden: Signal<u64>) -> impl View {
    tag_chip(series.label, series.color)
        .role(Role::Button)
        .label(series.label)
        .opacity(bind(move |runtime| {
            if is_hidden(hidden.get(runtime), index) {
                HIDDEN_OPACITY
            } else {
                1.0
            }
        }))
        .on_click(move |cx, _| hidden.update(cx, |mask| *mask ^= 1 << index))
}

fn painter<S>(
    source: S,
    series: Vec<Series>,
    range: YRange,
    hidden: Signal<u64>,
) -> impl FnMut(&mut Runtime, Size, &mut DrawList) + 'static
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[&[f32]])) + 'static,
{
    let mut decimated: Vec<Vec<ChartPoint>> = Vec::new();
    let mut lengths: Vec<usize> = Vec::new();
    let mut pixels = Vec::new();
    move |runtime, size, list| {
        let mask = hidden.get(runtime);
        source(runtime, &mut |lines| {
            decimated.resize_with(lines.len(), Vec::new);
            lengths.clear();
            for (index, (samples, out)) in lines.iter().zip(&mut decimated).enumerate() {
                lengths.push(samples.len());
                if is_hidden(mask, index) {
                    out.clear();
                } else {
                    decimate_m4(samples, size.width, out);
                }
            }
        });
        let (min, max) = range.resolve(decimated.iter().flatten().map(|point| point.value));
        for ((points, samples), entry) in decimated.iter().zip(&lengths).zip(&series) {
            let plot = Plot {
                size,
                samples: *samples,
                min,
                max,
            };
            pixels.clear();
            pixels.extend(points.iter().map(|point| plot.point(*point)));
            stroke_line(list, line_path(&pixels), entry.color.get(runtime));
        }
    }
}
