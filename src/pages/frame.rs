use crownui::kit::{card, use_theme};
use crownui::prelude::{
    Cx, Role, Semantic, Styled, View, ViewSeq, bind, component, scroll_view, text, vstack,
};

use crate::app::AppState;
use crate::util::motion::instant;

const TITLE_SIZE: f32 = 24.0;
const PANEL_TITLE_SIZE: f32 = 13.0;
const PANEL_PADDING: f32 = 16.0;
const PANEL_GAP: f32 = 12.0;

/// A page: its title over the given content, scrolling in the content card and named after
/// the page.
pub fn page_frame<C: ViewSeq + 'static>(
    cx: &mut Cx,
    state: AppState,
    title: &'static str,
    content: C,
) -> impl View + use<C> {
    let spacing = state.settings.current(cx).spacing;
    scroll_view(
        vstack((
            text(title)
                .text_size(TITLE_SIZE)
                .font_semibold()
                .role(Role::Heading),
            content,
        ))
        .gap(spacing.gap())
        .p(spacing.page_padding()),
    )
    .flex_1()
    .label(title)
    .transition(instant())
}

/// A titled card around one part of a page.
pub fn panel(title: &'static str, content: impl ViewSeq + 'static) -> impl View {
    component(move |cx| {
        let theme = use_theme(cx);
        card(
            vstack((
                text(title)
                    .text_size(PANEL_TITLE_SIZE)
                    .font_semibold()
                    .text_color(bind(move |runtime| {
                        theme.with(runtime, |theme| theme.text.muted)
                    })),
                content,
            ))
            .gap(PANEL_GAP)
            .p(PANEL_PADDING)
            .w_full(),
        )
        .self_stretch()
        .transition(instant())
    })
}
