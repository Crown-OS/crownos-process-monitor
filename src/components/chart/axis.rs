use crownui::ext::Prop;
use crownui::kit::use_theme;
use crownui::prelude::{Cx, IntoProp, Styled, View, bind, component, text, zstack};

const LABEL_SIZE: f32 = 11.0;
const LABEL_INSET: f32 = 4.0;

/// Text laid over a chart's corners: the value at the top edge and the times along the bottom.
pub struct AxisLabels {
    top: Prop<String>,
    start: Prop<String>,
    end: Prop<String>,
}

impl AxisLabels {
    pub fn new(
        top: impl IntoProp<String>,
        start: impl IntoProp<String>,
        end: impl IntoProp<String>,
    ) -> Self {
        Self {
            top: top.into_prop(),
            start: start.into_prop(),
            end: end.into_prop(),
        }
    }

    pub fn overlay(self) -> impl View {
        component(move |cx: &mut Cx| {
            let theme = use_theme(cx);
            let label = move |content: Prop<String>| {
                text(content)
                    .text_size(LABEL_SIZE)
                    .text_color(bind(move |runtime| {
                        theme.with(runtime, |theme| theme.text.muted)
                    }))
                    .absolute()
                    .pointer_events_none()
            };
            zstack((
                label(self.top).top(LABEL_INSET).left(LABEL_INSET),
                label(self.start).bottom(LABEL_INSET).left(LABEL_INSET),
                label(self.end).bottom(LABEL_INSET).right(LABEL_INSET),
            ))
            .absolute()
            .inset(0.0)
            .pointer_events_none()
        })
    }
}
