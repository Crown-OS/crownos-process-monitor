use crownui::kit::tag_chip;
use crownui::prelude::{
    Color, Cx, Memo, Runtime, Semantic, Styled, View, bind, hstack, text, vstack,
};

use super::super::sources::group_over;
use crate::app::AppState;
use crate::app::live::{FREE_CATEGORY, MEMORY_CATEGORIES};
use crate::components::chart::{Series, YRange, stacked_chart};
use crate::components::ring::{RingSegment, ring};
use crate::settings::ChartWindow;
use crate::util::format::bytes;
use crate::util::spacing;

const RING_DIAMETER: f32 = 150.0;
const STACK_HEIGHT: f32 = 150.0;
const LEGEND_SIZE: f32 = 12.0;
const LEGEND_GAP: f32 = 6.0;
const STACK_WINDOW: ChartWindow = ChartWindow::FifteenMinutes;

/// One colour per category; Free takes the control track so used memory stands out.
fn category_colors(cx: &mut Cx, state: AppState) -> [Memo<Color>; MEMORY_CATEGORIES.len()] {
    let theme = state.theme;
    std::array::from_fn(|index| {
        if index == FREE_CATEGORY {
            cx.memo(move |runtime| theme.with(runtime, |theme| theme.control.track))
        } else {
            state.series(cx, index)
        }
    })
}

fn legend(state: AppState, colors: [Memo<Color>; MEMORY_CATEGORIES.len()]) -> impl View {
    let live = state.live;
    let rows: Vec<_> = MEMORY_CATEGORIES
        .iter()
        .zip(colors)
        .enumerate()
        .map(|(index, (label, color))| {
            hstack((
                tag_chip(*label, color),
                text(bind(move |runtime| {
                    live.with(runtime, |snapshot| {
                        bytes(snapshot.memory.category_bytes[index])
                    })
                }))
                .text_size(LEGEND_SIZE),
            ))
            .gap(LEGEND_GAP)
            .items_center()
            .label(*label)
        })
        .collect();
    vstack(rows).gap(LEGEND_GAP)
}

/// The category ring with its legend, over the last fifteen minutes stacked by category.
pub fn breakdown(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let live = state.live;
    let colors = category_colors(cx, state);
    let hovered = cx.signal(None::<usize>);
    let segments = move |runtime: &mut Runtime, sink: &mut dyn FnMut(&[RingSegment])| {
        let mut shown: [RingSegment; MEMORY_CATEGORIES.len()] =
            std::array::from_fn(|index| RingSegment {
                value: 0.0,
                color: colors[index].get(runtime),
            });
        live.with(runtime, |snapshot| {
            for (segment, bytes) in shown.iter_mut().zip(snapshot.memory.category_bytes) {
                segment.value = bytes as f32;
            }
        });
        sink(&shown);
    };
    let donut = ring(segments, hovered)
        .diameter(RING_DIAMETER)
        .label(bind(move |runtime| {
            let picked = hovered.get(runtime);
            live.with(runtime, |snapshot| {
                let memory = &snapshot.memory;
                picked
                    .and_then(|index| memory.category_bytes.get(index))
                    .map_or_else(|| bytes(memory.used_bytes), |value| bytes(*value))
            })
        }))
        .sublabel(bind(move |runtime| {
            hovered
                .get(runtime)
                .and_then(|index| MEMORY_CATEGORIES.get(index))
                .map_or("used", |label| label)
                .to_owned()
        }));
    let layers: Vec<_> = MEMORY_CATEGORIES
        .iter()
        .zip(colors)
        .take(FREE_CATEGORY)
        .map(|(label, color)| Series { label, color })
        .collect();
    let over_time = stacked_chart(
        group_over(state, STACK_WINDOW, |snapshot| &snapshot.memory.categories),
        layers,
    )
    .range(YRange::PERCENT)
    .height(STACK_HEIGHT);
    vstack((
        hstack((donut, legend(state, colors)))
            .gap(spacing::GENEROUS)
            .items_center(),
        over_time,
    ))
    .gap(spacing::SECTION)
    .w_full()
}
