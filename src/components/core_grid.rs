use std::ops::Range;

use crownui::kit::{Theme, use_theme};
use crownui::prelude::{
    Color, CornerRadii, Memo, Point, Rect, Runtime, Signal, Size, Styled, View, bind, canvas,
    component, text, vstack,
};

use crate::util::spacing;

const HEATMAP_ABOVE: usize = 64;
const COLUMNS: u16 = 8;
const HEATMAP_COLUMNS: usize = 32;
const BAR_HEIGHT: f32 = 34.0;
const CELL_GAP: f32 = 6.0;
const HEATMAP_CELL: f32 = 12.0;
const LABEL_SIZE: f32 = 11.0;
const FULL_SCALE: f32 = 100.0;
const COLD_ALPHA: f32 = 0.12;

/// Logical CPUs that share a core type ("Performance", "Efficient", "SMT siblings"…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreGroup {
    pub label: String,
    pub cpus: Range<usize>,
}

/// A mini bar per logical CPU, grouped by core type; switches to one heatmap canvas when there
/// are more than 64 CPUs. `source` lends per-CPU utilisation in percent.
pub fn core_grid<S>(source: S, groups: Vec<CoreGroup>, tint: Memo<Color>) -> impl View
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + Clone + 'static,
{
    component(move |cx| {
        let theme = use_theme(cx);
        let total: usize = groups.iter().map(|group| group.cpus.len()).sum();
        if total > HEATMAP_ABOVE {
            return heatmap(source, tint, theme).boxed();
        }
        let sections: Vec<_> = groups
            .into_iter()
            .map(|group| group_view(group, source.clone(), tint, theme))
            .collect();
        vstack(sections).gap(CELL_GAP * 2.0).w_full().boxed()
    })
}

fn muted(theme: Signal<Theme>) -> impl crownui::prelude::IntoProp<Color> {
    bind(move |runtime| theme.with(runtime, |theme| theme.text.muted))
}

fn group_view<S>(
    group: CoreGroup,
    source: S,
    tint: Memo<Color>,
    theme: Signal<Theme>,
) -> impl View + use<S>
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + Clone + 'static,
{
    let cells: Vec<_> = group
        .cpus
        .clone()
        .map(|cpu| cell(cpu, source.clone(), tint, theme))
        .collect();
    vstack((
        text(group.label)
            .text_size(LABEL_SIZE)
            .text_color(muted(theme)),
        vstack(cells)
            .grid()
            .grid_cols(COLUMNS)
            .gap(CELL_GAP)
            .w_full(),
    ))
    .gap(CELL_GAP)
    .w_full()
}

fn cell<S>(cpu: usize, source: S, tint: Memo<Color>, theme: Signal<Theme>) -> impl View
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + 'static,
{
    let bar = canvas(move |runtime, size, list| {
        let mut value = 0.0;
        source(runtime, &mut |cores| {
            value = cores.get(cpu).copied().unwrap_or(0.0)
        });
        let fraction = (value / FULL_SCALE).clamp(0.0, 1.0);
        let track = theme.with(runtime, |theme| theme.control.track);
        let radii = CornerRadii::uniform(3.0);
        list.fill(Rect::new(Point::ZERO, size), radii, track);
        let height = fraction * size.height;
        list.fill(
            Rect::from_xywh(0.0, size.height - height, size.width, height),
            radii,
            tint.get(runtime),
        );
    })
    .w_full()
    .h(BAR_HEIGHT);
    vstack((
        bar,
        text(cpu.to_string())
            .text_size(LABEL_SIZE)
            .text_color(muted(theme)),
    ))
    .gap(spacing::HAIRLINE)
    .items_center()
}

fn heatmap<S>(source: S, tint: Memo<Color>, theme: Signal<Theme>) -> impl View
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + 'static,
{
    let rows = canvas(move |runtime, size, list| {
        let tint = tint.get(runtime);
        let track = theme.with(runtime, |theme| theme.control.track);
        source(runtime, &mut |cores| {
            let columns = HEATMAP_COLUMNS.min(cores.len().max(1));
            let cell = Size::new(
                (size.width - CELL_GAP / 2.0 * (columns - 1) as f32) / columns as f32,
                HEATMAP_CELL,
            );
            for (index, value) in cores.iter().enumerate() {
                let origin = Point::new(
                    (index % columns) as f32 * (cell.width + CELL_GAP / 2.0),
                    (index / columns) as f32 * (cell.height + CELL_GAP / 2.0),
                );
                let heat = (value / FULL_SCALE).clamp(0.0, 1.0);
                let color = track.lerp(tint, COLD_ALPHA + (1.0 - COLD_ALPHA) * heat);
                list.fill(Rect::new(origin, cell), CornerRadii::uniform(2.0), color);
            }
        });
    });
    rows.w_full().h(HEATMAP_CELL * 8.0)
}
