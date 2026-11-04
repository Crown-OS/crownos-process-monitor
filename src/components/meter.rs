use crownui::kit::use_theme;
use crownui::prelude::{Color, CornerRadii, Memo, Rect, Runtime, Styled, View, canvas, component};

const BAR_COUNT: usize = 4;
const BAR_GAP: f32 = 2.0;
const METER_WIDTH: f32 = 22.0;
const METER_HEIGHT: f32 = 16.0;

/// A small signal-strength gauge: `level` in `0..=1` lights up to four rising bars.
pub fn signal_meter<L>(level: L, tint: Memo<Color>) -> impl View
where
    L: Fn(&mut Runtime) -> f32 + 'static,
{
    component(move |cx| {
        let theme = use_theme(cx);
        canvas(move |runtime, size, list| {
            let lit = (level(runtime).clamp(0.0, 1.0) * BAR_COUNT as f32).round() as usize;
            let tint = tint.get(runtime);
            let track = theme.with(runtime, |theme| theme.control.track);
            let width = (size.width - BAR_GAP * (BAR_COUNT - 1) as f32) / BAR_COUNT as f32;
            for bar in 0..BAR_COUNT {
                let height = size.height * (bar + 1) as f32 / BAR_COUNT as f32;
                list.fill(
                    Rect::from_xywh(
                        bar as f32 * (width + BAR_GAP),
                        size.height - height,
                        width,
                        height,
                    ),
                    CornerRadii::uniform(1.5),
                    if bar < lit { tint } else { track },
                );
            }
        })
        .w(METER_WIDTH)
        .h(METER_HEIGHT)
        .shrink(0.0)
    })
}
