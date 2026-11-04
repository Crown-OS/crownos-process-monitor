use crownui::ext::{DrawList, NodeId};
use crownui::prelude::{
    Color, CornerRadii, Cx, Memo, Rect, Runtime, Signal, Size, Styled, View, canvas, zstack,
};

use super::hover::readout_label;
use super::scale::YRange;
use super::source::SeriesSource;
use crate::components::pointer::{Hover, pointer_tracker};

const DEFAULT_HEIGHT: f32 = 120.0;
const BAR_GAP: f32 = 4.0;
const BAR_RADIUS: f32 = 3.0;
const RESTING_ALPHA: f32 = 0.75;
const DIMMED_ALPHA: f32 = 0.4;

type BarReadout = Box<dyn Fn(&mut Runtime, usize, f32) -> String>;

/// Vertical bars, one per value: weekly totals, an hour strip, fan speeds. The bar under the
/// pointer is highlighted and its readout shown.
pub struct BarChart<S> {
    source: S,
    color: Memo<Color>,
    range: YRange,
    readout: Option<BarReadout>,
    height: f32,
}

pub fn bar_chart<S>(source: S, color: Memo<Color>) -> BarChart<S>
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + Clone + 'static,
{
    BarChart {
        source,
        color,
        range: YRange::AUTO,
        readout: None,
        height: DEFAULT_HEIGHT,
    }
}

impl<S> BarChart<S>
where
    S: SeriesSource,
{
    #[must_use]
    pub fn range(mut self, range: YRange) -> Self {
        self.range = range;
        self
    }

    /// Labels the hovered bar: `readout(runtime, index, value)`.
    #[must_use]
    pub fn readout(
        mut self,
        readout: impl Fn(&mut Runtime, usize, f32) -> String + 'static,
    ) -> Self {
        self.readout = Some(Box::new(readout));
        self
    }

    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}

fn hovered_bar(hover: Hover, count: usize) -> Option<usize> {
    let index = (hover.fraction_x() * count as f32) as usize;
    (count > 0).then(|| index.min(count - 1))
}

impl<S> View for BarChart<S>
where
    S: SeriesSource,
{
    fn build(self, cx: &mut Cx) -> NodeId {
        let hover = cx.signal(None);
        let surface = canvas(painter(self.source.clone(), self.color, self.range, hover))
            .absolute()
            .inset(0.0);
        let source = self.source;
        let label = self.readout.map(|readout| {
            readout_label(hover, move |runtime, hover| {
                let mut picked = None;
                source(runtime, &mut |values| {
                    picked = hovered_bar(hover, values.len())
                        .and_then(|index| Some((index, *values.get(index)?)));
                });
                picked.map_or_else(String::new, |(index, value)| readout(runtime, index, value))
            })
        });
        cx.build(
            pointer_tracker(hover, zstack((surface, label)).w_full().h(self.height))
                .w_full()
                .h(self.height),
        )
    }
}

fn painter<S>(
    source: S,
    color: Memo<Color>,
    range: YRange,
    hover: Signal<Option<Hover>>,
) -> impl FnMut(&mut Runtime, Size, &mut DrawList) + 'static
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + 'static,
{
    move |runtime, size, list| {
        let color = color.get(runtime);
        let hovered = hover.get(runtime);
        source(runtime, &mut |values| {
            let count = values.len();
            if count == 0 {
                return;
            }
            let highlighted = hovered.and_then(|hover| hovered_bar(hover, count));
            let (min, max) = range.resolve(values.iter().copied());
            let span = (max - min).max(f32::EPSILON);
            let width = ((size.width - BAR_GAP * (count - 1) as f32) / count as f32).max(1.0);
            for (index, value) in values.iter().enumerate() {
                let fraction = ((value - min) / span).clamp(0.0, 1.0);
                if !fraction.is_finite() {
                    continue;
                }
                let height = fraction * size.height;
                let alpha = match highlighted {
                    Some(picked) if picked == index => 1.0,
                    Some(_) => DIMMED_ALPHA,
                    None => RESTING_ALPHA,
                };
                list.fill(
                    Rect::from_xywh(
                        index as f32 * (width + BAR_GAP),
                        size.height - height,
                        width,
                        height,
                    ),
                    CornerRadii::uniform(BAR_RADIUS.min(width / 2.0)),
                    color.with_alpha_factor(alpha),
                );
            }
        });
    }
}
