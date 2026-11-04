use crownui::kit::use_theme;
use crownui::prelude::{
    AnyView, Role, Semantic, Styled, View, ViewSeq, bind, component, hstack, text, vstack,
};

use crate::util::spacing;

const SECTION_TITLE_SIZE: f32 = 13.0;
const FIELD_SIZE: f32 = 12.0;
const LABEL_WIDTH: f32 = 118.0;
const FIELD_GAP: f32 = 6.0;

/// One "label: value" line of a section; empty values are left out.
pub struct Field {
    pub label: &'static str,
    pub value: String,
}

/// Collects a section's lines, skipping the ones with nothing to say.
#[derive(Default)]
pub struct Fields(Vec<Field>);

impl Fields {
    pub fn add(&mut self, label: &'static str, value: impl Into<String>) -> &mut Self {
        let value = value.into();
        if !value.is_empty() {
            self.0.push(Field { label, value });
        }
        self
    }

    pub fn add_some(&mut self, label: &'static str, value: Option<String>) -> &mut Self {
        if let Some(value) = value {
            self.add(label, value);
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_views(self) -> Vec<AnyView> {
        self.0.into_iter().map(field_row).collect()
    }
}

fn field_row(field: Field) -> AnyView {
    component(move |cx| {
        let theme = use_theme(cx);
        hstack((
            text(field.label)
                .text_size(FIELD_SIZE)
                .text_color(bind(move |runtime| {
                    theme.with(runtime, |theme| theme.text.muted)
                }))
                .w(LABEL_WIDTH)
                .shrink(0.0),
            text(field.value).text_size(FIELD_SIZE).flex_1().min_w(0.0),
        ))
        .gap(FIELD_GAP)
        .items_start()
        .w_full()
        .role(Role::Group)
        .label(field.label)
    })
    .boxed()
}

/// A titled block of the drawer, named after its title for screen readers and tests.
pub fn section(title: &'static str, content: impl ViewSeq + 'static) -> AnyView {
    component(move |cx| {
        let theme = use_theme(cx);
        vstack((
            text(title)
                .text_size(SECTION_TITLE_SIZE)
                .font_semibold()
                .text_color(bind(move |runtime| {
                    theme.with(runtime, |theme| theme.text.muted)
                }))
                .role(Role::Heading),
            vstack(content).gap(FIELD_GAP).w_full(),
        ))
        .gap(spacing::COMFORTABLE)
        .w_full()
        .role(Role::Group)
        .label(title)
    })
    .boxed()
}

/// A section of plain fields, or nothing when every field is empty.
pub fn field_section(title: &'static str, fields: Fields) -> Option<AnyView> {
    (!fields.is_empty()).then(|| section(title, fields.into_views()))
}

pub fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}
