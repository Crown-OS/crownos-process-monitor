use crownui::kit::{ButtonOptions, ButtonSize, ButtonVariant, button, icons};
use crownui::prelude::{AnyView, Listeners, Signal, Styled, View, hstack, vstack};
use monitor_core::ProcessKey;
use monitor_engine::system::ProcessTable;
use monitor_engine::system::inspect::ProcessDetails;

use crate::inspector::fields::section;
use crate::util::spacing;

fn link(name: &str, key: ProcessKey, target: Signal<Option<ProcessKey>>) -> AnyView {
    button(format!("{name} ({})", key.pid.get()))
        .variant(ButtonVariant::Ghost)
        .control_size(ButtonSize::Small)
        .leading_icon(icons::ARROW_RIGHT)
        .on_click(move |cx, _| target.set(cx, Some(key)))
        .boxed()
}

/// The parent, a link that re-targets the drawer.
pub fn parent(
    details: &ProcessDetails,
    table: &ProcessTable,
    target: Signal<Option<ProcessKey>>,
) -> Option<AnyView> {
    let row = table
        .index_of(details.identity.parent)
        .and_then(|index| table.processes.get(index))?;
    Some(section("Parent", hstack(link(&row.name, row.key, target))))
}

/// The direct children, each a link.
pub fn children(details: &ProcessDetails, target: Signal<Option<ProcessKey>>) -> Option<AnyView> {
    let links: Vec<AnyView> = details
        .children
        .iter()
        .map(|child| link(&child.name, child.key, target))
        .collect();
    (!links.is_empty()).then(|| {
        section(
            "Children",
            vstack(links).items_start().gap(spacing::HAIRLINE),
        )
    })
}
