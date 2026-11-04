use std::sync::Arc;

use crownui::kit::{
    MenuRowOptions, context_menu_at, icons, menu_item, menu_item_selected, menu_separator,
    menu_submenu,
};
use crownui::prelude::{AnyView, Cx, Listeners, Point, Signal, Styled, View, component, vstack};
use monitor_engine::system::{ProcessAction, ProcessState};

use super::affinity::AffinityDialog;
use super::alert::{AlertRequest, AlertWish};
use super::perform::RowAction;
use super::prompt::ActionPrompt;
use super::target::representative;
use crate::app::AppState;
use crate::app::live::AppList;
use crate::components::app_table::{AppCategory, RowId, TableState};

/// The renice choices, highest priority first. Below the current nice needs the helper.
pub const NICE_LEVELS: [(i32, &str); 5] = [
    (-10, "Highest (−10)"),
    (-5, "High (−5)"),
    (0, "Normal (0)"),
    (10, "Low (10)"),
    (19, "Lowest (19)"),
];

/// Everything the row menu acts through.
#[derive(Clone, Copy)]
pub struct MenuContext {
    pub state: AppState,
    pub table: TableState,
    pub prompt: ActionPrompt,
    pub inspect: Signal<Option<RowId>>,
    /// Window position of the last press over the table, where the renice menu opens.
    pub pointer: Signal<Point>,
    pub renice_at: Signal<Option<Point>>,
    pub renice_row: Signal<Option<RowId>>,
    pub affinity: AffinityDialog,
}

/// What the menu shows for the row it opened on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowSummary {
    pub name: Arc<str>,
    pub path: Option<Arc<str>>,
    pub command_line: Option<Arc<str>>,
    pub is_app: bool,
    pub ended: bool,
    pub pausable: bool,
    /// A stopped process, or an app whose scope is frozen.
    pub stopped: bool,
    pub nice: i32,
    pub pinned: bool,
}

impl RowSummary {
    pub fn read(apps: &AppList, row: RowId) -> Option<Self> {
        let app = apps.app_by_key(row.app)?;
        let process = representative(apps, row);
        let is_app = row.process.is_none();
        let facts = process.map(|process| &process.facts);
        Some(Self {
            name: match (is_app, process) {
                (false, Some(process)) => Arc::clone(&process.name),
                _ => Arc::clone(&app.name),
            },
            path: facts.and_then(|facts| facts.path.clone()),
            command_line: facts
                .map(|facts| Arc::clone(&facts.command_line))
                .filter(|line| !line.is_empty()),
            is_app,
            ended: app.facts.category == AppCategory::Ended,
            pausable: !is_app || app.facts.pausable,
            stopped: if is_app {
                app.facts.frozen
            } else {
                facts.is_some_and(|facts| facts.state == ProcessState::Stopped)
            },
            nice: facts.map_or(0, |facts| facts.nice),
            pinned: false,
        })
    }

    /// Pause for a running process or app, Resume for a stopped process or frozen app scope.
    pub fn pause_actions(&self) -> &'static [ProcessAction] {
        match (self.pausable, self.stopped) {
            (false, _) => &[],
            (true, true) => &[ProcessAction::Resume],
            (true, false) => &[ProcessAction::Pause],
        }
    }
}

fn picked(cx: &mut Cx, target: Signal<Option<RowId>>) -> Option<RowId> {
    target.with_untracked(cx, |target| *target)
}

fn copy_row(label: &'static str, text: Arc<str>) -> AnyView {
    menu_item(label)
        .leading_icon(icons::COPY)
        .on_click(move |cx, _| cx.copy_to_clipboard(text.to_string()))
        .boxed()
}

fn action_row(
    label: &'static str,
    icon: &'static str,
    context: MenuContext,
    row: RowId,
    action: RowAction,
) -> AnyView {
    menu_item(label)
        .leading_icon(icon)
        .on_click(move |cx, _| context.prompt.request(cx, row, action))
        .boxed()
}

fn alert_row(label: &'static str, wish: AlertWish, context: MenuContext, row: RowId) -> AnyView {
    menu_item(label)
        .leading_icon(icons::BELL)
        .on_click(move |cx, _| {
            let request = context.state.live.with_untracked(cx, |live| {
                let app = live.apps.app_by_key(row.app)?;
                let process = representative(&live.apps, row).filter(|_| row.process.is_some());
                Some(AlertRequest {
                    wish,
                    app: app.facts.app_key.clone(),
                    process: process.map(|process| process.facts.key(process.pid)),
                    name: process.map_or_else(
                        || Arc::clone(&app.name),
                        |process| Arc::clone(&process.name),
                    ),
                })
            });
            if request.is_some() {
                context.state.alert_request.set(cx, request);
            }
        })
        .boxed()
}

fn menu_rows(context: MenuContext, row: RowId, summary: RowSummary) -> Vec<AnyView> {
    let mut rows = Vec::with_capacity(16);
    if !summary.ended {
        rows.push(
            menu_item("Inspect")
                .leading_icon(icons::INFO)
                .shortcut_hint("Enter")
                .on_click(move |cx, _| context.inspect.set(cx, Some(row)))
                .boxed(),
        );
    }
    rows.push(copy_row("Copy Name", Arc::clone(&summary.name)));
    if let Some(path) = summary.path.clone() {
        rows.push(copy_row("Copy Path", path));
    }
    if let Some(line) = summary.command_line.clone() {
        rows.push(copy_row("Copy Command Line", line));
    }
    if summary.ended {
        return rows;
    }
    if summary.is_app {
        rows.push(
            menu_item(if summary.pinned {
                "Unpin"
            } else {
                "Pin to Top"
            })
            .leading_icon(icons::PIN)
            .on_click(move |cx, _| context.table.toggle_pinned(cx, row.app))
            .boxed(),
        );
    }
    rows.push(menu_separator().boxed());
    rows.push(action_row(
        "Quit…",
        icons::CIRCLE_X,
        context,
        row,
        RowAction::Signal(ProcessAction::Quit),
    ));
    rows.push(action_row(
        "Force Quit…",
        icons::OCTAGON_X,
        context,
        row,
        RowAction::Signal(ProcessAction::ForceQuit),
    ));
    for action in summary.pause_actions() {
        let (label, icon) = match action {
            ProcessAction::Pause => ("Pause…", icons::PAUSE),
            _ => ("Resume", icons::PLAY),
        };
        rows.push(action_row(
            label,
            icon,
            context,
            row,
            RowAction::Signal(*action),
        ));
    }
    rows.push(
        menu_submenu("Renice")
            .leading_icon(icons::GAUGE)
            .on_click(move |cx, _| {
                let at = context.pointer.get(cx);
                context.renice_row.set(cx, Some(row));
                context.renice_at.set(cx, Some(at));
            })
            .boxed(),
    );
    if !summary.is_app {
        rows.push(
            menu_item("CPU Affinity…")
                .leading_icon(icons::CPU)
                .on_click(move |cx, _| context.affinity.show(cx, row))
                .boxed(),
        );
    }
    rows.push(menu_separator().boxed());
    rows.push(alert_row("Alert When…", AlertWish::AlertWhen, context, row));
    rows.push(alert_row(
        "Don't Alert About…",
        AlertWish::DontAlert,
        context,
        row,
    ));
    rows
}

/// The row menu: built when it opens, from the row it opened on.
pub fn row_menu(
    context: MenuContext,
) -> impl FnMut(Signal<Option<RowId>>) -> Vec<AnyView> + 'static {
    move |target| {
        vec![
            component(move |cx| {
                let summary = picked(cx, target).and_then(|row| {
                    let mut summary = context
                        .state
                        .live
                        .with_untracked(cx, |live| RowSummary::read(&live.apps, row))?;
                    summary.pinned = context
                        .table
                        .pinned
                        .with_untracked(cx, |keys| keys.contains(&row.app));
                    Some((row, summary))
                });
                let rows = summary
                    .map_or_else(Vec::new, |(row, summary)| menu_rows(context, row, summary));
                vstack(rows).items_stretch().self_stretch()
            })
            .boxed(),
        ]
    }
}

/// The Renice submenu, opened beside the row menu.
pub fn renice_menu(context: MenuContext) -> impl View {
    context_menu_at(context.renice_at, move || {
        component(move |cx| {
            let row = picked(cx, context.renice_row);
            let current = row.and_then(|row| {
                context
                    .state
                    .live
                    .with_untracked(cx, |live| RowSummary::read(&live.apps, row))
                    .map(|summary| summary.nice)
            });
            let rows: Vec<AnyView> = row
                .map(|row| {
                    NICE_LEVELS
                        .iter()
                        .map(|&(nice, label)| {
                            let item = if current == Some(nice) {
                                menu_item_selected(label).leading_icon(icons::CHECK)
                            } else {
                                menu_item(label)
                            };
                            item.on_click(move |cx, _| {
                                context.prompt.request(cx, row, RowAction::Renice(nice));
                            })
                            .boxed()
                        })
                        .collect()
                })
                .unwrap_or_default();
            vstack(rows).items_stretch().self_stretch()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_offers_what_applies() {
        let process = RowSummary {
            pausable: true,
            ..RowSummary::default()
        };
        assert_eq!(process.pause_actions(), [ProcessAction::Pause]);
        let stopped = RowSummary {
            stopped: true,
            ..process.clone()
        };
        assert_eq!(stopped.pause_actions(), [ProcessAction::Resume]);
        let scoped_app = RowSummary {
            is_app: true,
            ..process
        };
        assert_eq!(scoped_app.pause_actions(), [ProcessAction::Pause]);
        let frozen_app = RowSummary {
            stopped: true,
            ..scoped_app
        };
        assert_eq!(frozen_app.pause_actions(), [ProcessAction::Resume]);
        assert!(RowSummary::default().pause_actions().is_empty());
    }
}
