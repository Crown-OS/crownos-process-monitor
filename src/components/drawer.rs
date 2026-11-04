use crownui::kit::{ButtonOptions, ButtonVariant, icon_button, icons, use_theme};
use crownui::prelude::{
    Cx, IntoProp, Listeners, Role, Semantic, Signal, Spring, Styled, View, ViewSeq, bind,
    component, hstack, scroll_view, show, text, vstack,
};

const DRAWER_WIDTH: f32 = 380.0;
const DRAWER_GAP: f32 = 8.0;
const HEADER_PADDING: f32 = 14.0;
const TITLE_SIZE: f32 = 16.0;
const SECTION_GAP: f32 = 18.0;
const DRAWER_RADIUS: f32 = 12.0;

/// A right side sheet beside `content`: while `open`, the content shrinks to make room instead
/// of being covered. The header carries the title, a share button and a close button; the
/// `sections` scroll below it.
pub fn drawer<T, F, Sections, Content>(
    open: Signal<bool>,
    title: T,
    mut sections: F,
    on_share: impl Fn(&mut Cx) + Clone + 'static,
    content: Content,
) -> impl View
where
    T: IntoProp<String> + Clone + 'static,
    F: FnMut() -> Sections + 'static,
    Sections: ViewSeq + 'static,
    Content: View,
{
    let sheet = move || {
        let title = title.clone();
        let on_share = on_share.clone();
        let sections = sections();
        component(move |cx| {
            let theme = use_theme(cx);
            vstack((
                hstack((
                    text(title.clone())
                        .text_size(TITLE_SIZE)
                        .font_semibold()
                        .truncate()
                        .flex_1()
                        .role(Role::Heading),
                    icon_button(icons::SHARE_2, "Share")
                        .variant(ButtonVariant::Ghost)
                        .on_click(move |cx, _| on_share(cx)),
                    icon_button(icons::X, "Close")
                        .variant(ButtonVariant::Ghost)
                        .on_click(move |cx, _| open.set(cx, false)),
                ))
                .gap(DRAWER_GAP)
                .items_center()
                .p(HEADER_PADDING),
                scroll_view(vstack(sections).gap(SECTION_GAP).p(HEADER_PADDING))
                    .flex_1()
                    .label(title),
            ))
            .w(DRAWER_WIDTH)
            .h_full()
            .shrink(0.0)
            .rounded(DRAWER_RADIUS)
            .bg(bind(move |runtime| {
                theme.with(runtime, |theme| theme.surface.raised)
            }))
            .border(1.0)
            .border_color(bind(move |runtime| {
                theme.with(runtime, |theme| theme.surface.border)
            }))
            .enter(|style| style.translate_x(DRAWER_WIDTH).opacity(0.0))
            .exit(|style| style.translate_x(DRAWER_WIDTH).opacity(0.0))
            .transition(Spring::snappy())
        })
    };
    hstack((
        vstack(content).flex_1().min_w(0.0).h_full(),
        show(open, sheet, || vstack(())),
    ))
    .gap(DRAWER_GAP)
    .w_full()
    .h_full()
}
