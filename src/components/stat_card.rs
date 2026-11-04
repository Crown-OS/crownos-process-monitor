use crownui::ext::Prop;
use crownui::kit::{card, use_theme};
use crownui::prelude::{
    Color, Cx, IntoProp, Listeners, Memo, Role, Runtime, Semantic, Styled, View, bind, component,
    hstack, text, vstack,
};

use super::chart::area_chart;
use super::stat_strip::Figure;
use crate::util::icon::tinted_icon;
use crate::util::motion::instant;
use crate::util::spacing;

const HEADLINE_SIZE: f32 = 26.0;
const FIGURE_SIZE: f32 = 14.0;
const TITLE_SIZE: f32 = 13.0;
const SPARKLINE_HEIGHT: f32 = 44.0;
const CARD_PADDING: f32 = 14.0;
const CARD_GAP: f32 = 10.0;
const ICON_SIZE: f32 = 16.0;
const CARD_MIN_WIDTH: f32 = 220.0;

/// What an Overview card shows.
pub struct StatCardContent {
    pub title: &'static str,
    pub icon: &'static str,
    pub tint: Memo<Color>,
    pub headline: Prop<String>,
    pub figures: Vec<Figure>,
}

impl StatCardContent {
    pub fn new(
        title: &'static str,
        icon: &'static str,
        tint: Memo<Color>,
        headline: impl IntoProp<String>,
    ) -> Self {
        Self {
            title,
            icon,
            tint,
            headline: headline.into_prop(),
            figures: Vec::new(),
        }
    }

    #[must_use]
    pub fn figure(mut self, figure: Figure) -> Self {
        self.figures.push(figure);
        self
    }
}

fn card_body<S>(cx: &mut Cx, content: StatCardContent, sparkline: S) -> impl View + use<S>
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + Clone + 'static,
{
    let theme = use_theme(cx);
    let StatCardContent {
        title,
        icon,
        tint,
        headline,
        figures,
    } = content;
    let figures: Vec<_> = figures
        .into_iter()
        .map(|figure| figure.view(cx, FIGURE_SIZE))
        .collect();
    let muted = bind(move |runtime| theme.with(runtime, |theme| theme.text.muted));
    vstack((
        hstack((
            tinted_icon(icon, tint, ICON_SIZE),
            text(title)
                .text_size(TITLE_SIZE)
                .text_color(muted)
                .font_medium(),
        ))
        .gap(spacing::SNUG)
        .items_center(),
        text(headline).text_size(HEADLINE_SIZE).font_semibold(),
        hstack(figures).gap(CARD_GAP * 1.5),
        area_chart(sparkline, tint).height(SPARKLINE_HEIGHT),
    ))
    .gap(CARD_GAP)
    .p(CARD_PADDING)
}

/// A clickable Overview card: title, headline, a few figures and a sparkline.
pub fn stat_card<S>(
    content: StatCardContent,
    sparkline: S,
    on_click: impl Fn(&mut Cx) + 'static,
) -> impl View
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + Clone + 'static,
{
    component(move |cx| {
        let title = content.title;
        card(card_body(cx, content, sparkline))
            .min_w(CARD_MIN_WIDTH)
            .flex_1()
            .role(Role::Button)
            .label(title)
            .on_click(move |cx, _| on_click(cx))
            .hover(|style| style.scale(1.01))
            .transition(instant())
    })
}

/// The same card as a read-only tile, for figures that have no page of their own.
pub fn stat_tile<S>(content: StatCardContent, sparkline: S) -> impl View
where
    S: Fn(&mut Runtime, &mut dyn FnMut(&[f32])) + Clone + 'static,
{
    component(move |cx| {
        let title = content.title;
        card(card_body(cx, content, sparkline))
            .min_w(CARD_MIN_WIDTH)
            .flex_1()
            .role(Role::Group)
            .label(title)
            .transition(instant())
    })
}
