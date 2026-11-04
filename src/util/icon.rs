use crownui::prelude::{Color, IntoProp, Styled, View, icon};

const VIEW_BOX: u32 = 24;
const STROKE_WIDTH: f32 = 2.0;

/// A `size`-pixel Lucide glyph from `kit::icons`, painted in `tint` instead of a theme colour.
pub fn tinted_icon(body: &'static str, tint: impl IntoProp<Color>, size: f32) -> impl View {
    let source = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{VIEW_BOX}" height="{VIEW_BOX}" viewBox="0 0 {VIEW_BOX} {VIEW_BOX}" fill="none" stroke="#000000" stroke-width="{STROKE_WIDTH}" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"##
    );
    icon(source, tint).size(size).shrink(0.0)
}
