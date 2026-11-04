use crownui::ext::{DrawCommand, DrawList, Paint, Path, Stroke};
use crownui::kit::GradientStops;
use crownui::prelude::Color;

const FILL_TOP_ALPHA: f32 = 0.35;
const LINE_WIDTH: f32 = 1.75;

/// The chart fill: the tint fading out towards `bottom`.
pub fn fade_fill(color: Color, top: f32, bottom: f32) -> Paint {
    GradientStops::new(
        color.with_alpha_factor(FILL_TOP_ALPHA),
        color.with_alpha_factor(0.0),
    )
    .vertical(top, bottom)
}

pub fn fill(list: &mut DrawList, path: Path, paint: Paint) {
    if !path.commands.is_empty() {
        list.push(DrawCommand::FillPath(path, paint));
    }
}

pub fn stroke_line(list: &mut DrawList, path: Path, color: Color) {
    if !path.commands.is_empty() {
        list.push(DrawCommand::StrokePath(
            path,
            Stroke::rounded(LINE_WIDTH),
            Paint::solid(color),
        ));
    }
}

pub fn stroke_hairline(list: &mut DrawList, path: Path, color: Color) {
    if !path.commands.is_empty() {
        list.push(DrawCommand::StrokePath(
            path,
            Stroke::new(1.0),
            Paint::solid(color),
        ));
    }
}
