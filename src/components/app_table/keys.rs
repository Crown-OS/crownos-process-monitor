use crownui::ext::{Key, KeyboardEvent, NamedKey};
use crownui::prelude::{Cx, Runtime, ScrollHandle};

use super::data::{RowId, RowRef, TableRows};
use super::row::RowContext;

enum Step {
    Up,
    Down,
}

/// Arrow keys move the selection, Right/Left expand or collapse an app, Enter activates the row
/// (or toggles an app without an activate handler), Delete asks to quit it, Shift+F10 or the
/// Menu key opens its context menu. Returns whether the key was handled.
pub(super) fn handle_key<S, D>(
    cx: &mut Cx,
    event: &KeyboardEvent,
    context: &RowContext<S, D>,
    handle: ScrollHandle,
) -> bool
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    let modifiers = event.modifiers;
    match event.key {
        Key::Named(NamedKey::ArrowDown) => move_selection(cx, context, handle, Step::Down),
        Key::Named(NamedKey::ArrowUp) => move_selection(cx, context, handle, Step::Up),
        Key::Named(NamedKey::ArrowRight) => expand_selected(cx, context, true),
        Key::Named(NamedKey::ArrowLeft) => expand_selected(cx, context, false),
        Key::Named(NamedKey::Enter) => activate_selected(cx, context),
        Key::Named(NamedKey::Delete) => delete_selected(cx, context),
        Key::Named(NamedKey::ContextMenu) => open_menu(cx, context),
        Key::Named(NamedKey::F(10)) if modifiers.shift_key => open_menu(cx, context),
        _ => return false,
    }
    true
}

fn selected_id<S, D>(cx: &mut Cx, context: &RowContext<S, D>) -> Option<RowId>
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    let (_, row) = selected_row(cx, context)?;
    let project = context.project;
    Some(
        context
            .data
            .with_untracked(cx, |source| project(source).row_id(row)),
    )
}

fn delete_selected<S, D>(cx: &mut Cx, context: &RowContext<S, D>)
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    if let (Some(id), Some(delete)) = (selected_id(cx, context), &context.delete) {
        delete(cx, id);
    }
}

fn open_menu<S, D>(cx: &mut Cx, context: &RowContext<S, D>)
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    if let Some(id) = selected_id(cx, context) {
        context.menu_target.set(cx, Some(id));
        let at = context.pointer.get(cx);
        context.menu_at.set(cx, Some(at));
    }
}

fn selected_row<S, D>(runtime: &Runtime, context: &RowContext<S, D>) -> Option<(usize, RowRef)>
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    let selected = context.state.selected.with_untracked(runtime, |id| *id)?;
    let project = context.project;
    context.data.with_untracked(runtime, |source| {
        let data = project(source);
        context.model.with(runtime, |model| {
            let index = model.position(data, selected)?;
            Some((index, *model.rows().get(index)?))
        })
    })
}

fn row_id_at<S, D>(runtime: &Runtime, context: &RowContext<S, D>, index: usize) -> Option<RowId>
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    let project = context.project;
    context.data.with_untracked(runtime, |source| {
        context.model.with(runtime, |model| {
            model
                .rows()
                .get(index)
                .map(|row| project(source).row_id(*row))
        })
    })
}

fn move_selection<S, D>(cx: &mut Cx, context: &RowContext<S, D>, handle: ScrollHandle, step: Step)
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    let count = context.model.with(cx, |model| model.rows().len());
    if count == 0 {
        return;
    }
    let target = match (selected_row(cx, context), step) {
        (None, _) => 0,
        (Some((index, _)), Step::Down) => (index + 1).min(count - 1),
        (Some((index, _)), Step::Up) => index.saturating_sub(1),
    };
    if let Some(id) = row_id_at(cx, context, target) {
        context.state.selected.set(cx, Some(id));
        handle.ensure_visible(cx, target);
    }
}

fn expand_selected<S, D>(cx: &mut Cx, context: &RowContext<S, D>, expanded: bool)
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    if let Some((_, RowRef::App(app))) = selected_row(cx, context) {
        let project = context.project;
        let key = context
            .data
            .with_untracked(cx, |source| project(source).app_key(app));
        context.state.set_expanded(cx, key, expanded);
    }
}

fn activate_selected<S, D>(cx: &mut Cx, context: &RowContext<S, D>)
where
    S: 'static,
    D: TableRows + ?Sized + 'static,
{
    let Some((_, row)) = selected_row(cx, context) else {
        return;
    };
    let project = context.project;
    let id = context
        .data
        .with_untracked(cx, |source| project(source).row_id(row));
    match &context.activate {
        Some(activate) => activate(cx, id),
        None if matches!(row, RowRef::App(_)) => context.state.toggle_expanded(cx, id.app),
        None => {}
    }
}
