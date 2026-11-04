use crownui::ext::{DrawList, NodeId};
use crownui::prelude::{Cx, Point, Runtime, Size, Styled, View, canvas};

use super::decimate::column_means;
use super::lines::Series;
use super::paint::{fill, stroke_line};
use super::scale::{Plot, YRange};
use super::source::MultiSeriesSource;
use super::trace::{band_path, line_path};

const DEFAULT_HEIGHT: f32 = 160.0;
const PIXELS_PER_COLUMN: f32 = 2.0;
const LAYER_ALPHA: f32 = 0.55;

/// Layers stacked on each other, bottom first: CPU states, memory categories. `source` lends one
/// slice per layer, in the order of `layers`.
pub struct StackedChart<S> {
    source: S,
    layers: Vec<Series>,
    range: YRange,
    height: f32,
}

pub fn stacked_chart<S>(source: S, layers: Vec<Series>) -> StackedChart<S>
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[&[f32]])) + Clone + 'static,
{
    StackedChart {
        source,
        layers,
        range: YRange::AUTO,
        height: DEFAULT_HEIGHT,
    }
}

impl<S> StackedChart<S>
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
}

impl<S> View for StackedChart<S>
where
    S: MultiSeriesSource,
{
    fn build(self, cx: &mut Cx) -> NodeId {
        cx.build(
            canvas(painter(self.source, self.layers, self.range))
                .w_full()
                .h(self.height),
        )
    }
}

#[derive(Default)]
struct StackBuffers {
    means: Vec<Vec<f32>>,
    running: Vec<f32>,
    lower: Vec<Point>,
    upper: Vec<Point>,
}

fn painter<S>(
    source: S,
    layers: Vec<Series>,
    range: YRange,
) -> impl FnMut(&mut Runtime, Size, &mut DrawList) + 'static
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[&[f32]])) + 'static,
{
    let mut buffers = StackBuffers::default();
    move |runtime, size, list| {
        let columns = ((size.width / PIXELS_PER_COLUMN) as usize).max(2);
        let StackBuffers {
            means,
            running,
            lower,
            upper,
        } = &mut buffers;
        source(runtime, &mut |series| {
            means.resize_with(series.len(), Vec::new);
            for (samples, out) in series.iter().zip(means.iter_mut()) {
                column_means(samples, columns, out);
            }
        });
        let count = means.iter().map(Vec::len).min().unwrap_or(0);
        running.clear();
        running.resize(count, 0.0);
        for layer in means.iter() {
            for (total, value) in running.iter_mut().zip(layer) {
                *total += value;
            }
        }
        let (min, max) = range.resolve(running.iter().copied());
        let plot = Plot {
            size,
            samples: count,
            min,
            max,
        };
        running.fill(0.0);
        lower.clear();
        lower.extend((0..count).map(|index| Point::new(plot.x(index as f32), size.height)));
        for (layer, entry) in means.iter().zip(&layers) {
            upper.clear();
            for (index, (total, value)) in running.iter_mut().zip(layer).enumerate() {
                *total += value;
                upper.push(Point::new(plot.x(index as f32), plot.y(*total)));
            }
            let color = entry.color.get(runtime);
            fill(
                list,
                band_path(upper, lower),
                color.with_alpha_factor(LAYER_ALPHA).into(),
            );
            stroke_line(list, line_path(upper), color);
            std::mem::swap(upper, lower);
        }
    }
}
