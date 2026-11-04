use crownui::kit::{Theme, use_theme};
use crownui::prelude::{
    Color, CornerRadii, Memo, Point, Rect, Runtime, Size, Styled, View, bind, canvas, component,
    hstack, spacer, text, vstack,
};

use crate::util::format::{write_bytes, write_percent};
use crate::util::spacing;

const BAR_HEIGHT: f32 = 8.0;
const WARNING_FRACTION: f32 = 0.75;
const CRITICAL_FRACTION: f32 = 0.9;
const LABEL_SIZE: f32 = 12.0;

/// Bytes in use out of a total.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Capacity {
    pub used: u64,
    pub total: u64,
}

impl Capacity {
    pub fn fraction(self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.used as f64 / self.total as f64).clamp(0.0, 1.0) as f32
        }
    }
}

fn fill_color(theme: &Theme, tint: Color, fraction: f32) -> Color {
    if fraction >= CRITICAL_FRACTION {
        theme.status.danger
    } else if fraction >= WARNING_FRACTION {
        theme.status.warning
    } else {
        tint
    }
}

/// "412 GB of 1 TB" over a bar that turns amber at 75 % and red at 90 %.
pub fn capacity_bar<C>(usage: C, tint: Memo<Color>) -> impl View
where
    C: Fn(&mut Runtime) -> Capacity + Clone + 'static,
{
    component(move |cx| {
        let theme = use_theme(cx);
        let usage_text = usage.clone();
        let percent_text = usage.clone();
        let bar = canvas(move |runtime, size, list| {
            let fraction = usage(runtime).fraction();
            let tint = tint.get(runtime);
            let (track, fill) = theme.with(runtime, |theme| {
                (theme.control.track, fill_color(theme, tint, fraction))
            });
            let radii = CornerRadii::uniform(size.height / 2.0);
            list.fill(Rect::new(Point::ZERO, size), radii, track);
            let filled = Size::new((fraction * size.width).max(size.height), size.height);
            if fraction > 0.0 {
                list.fill(Rect::new(Point::ZERO, filled), radii, fill);
            }
        })
        .w_full()
        .h(BAR_HEIGHT);
        let muted = bind(move |runtime| theme.with(runtime, |theme| theme.text.muted));
        vstack((
            bar,
            hstack((
                text(bind(move |runtime| {
                    let capacity = usage_text(runtime);
                    let mut label = String::new();
                    write_bytes(&mut label, capacity.used);
                    label.push_str(" of ");
                    write_bytes(&mut label, capacity.total);
                    label
                }))
                .text_size(LABEL_SIZE),
                spacer(),
                text(bind(move |runtime| {
                    let mut label = String::new();
                    write_percent(&mut label, percent_text(runtime).fraction() * 100.0);
                    label
                }))
                .text_size(LABEL_SIZE)
                .text_color(muted),
            )),
        ))
        .gap(spacing::SNUG)
        .w_full()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractions_clamp_and_survive_an_empty_total() {
        assert_eq!(Capacity { used: 5, total: 0 }.fraction(), 0.0);
        assert_eq!(
            Capacity {
                used: 50,
                total: 100
            }
            .fraction(),
            0.5
        );
        assert_eq!(
            Capacity {
                used: 150,
                total: 100
            }
            .fraction(),
            1.0
        );
    }
}
