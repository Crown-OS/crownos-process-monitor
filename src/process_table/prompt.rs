use std::sync::Arc;

use crownui::kit::Toasts;
use crownui::prelude::{Cx, Signal, Store, View};
use monitor_engine::system::ProcessAction;
use monitor_engine::{HelperAction, HelperOutcome, Snapshot};

use super::assess::{Assessment, NextStep, assess};
use super::escalate::{command, helper_actions, helper_message};
use super::perform::{RowAction, outcome_message, run};
use super::target::{ActionTarget, process_count};
use crate::app::{AppState, EngineLink};
use crate::components::app_table::RowId;
use crate::components::confirm::{ConfirmChoice, Confirmation, confirm_dialog};
use crate::util::broadcast::follow_broadcast;

const GONE_MESSAGE: &str = "That process has already ended";

/// An action handed to the helper after a permission error, awaiting its answer.
struct Escalated {
    action: HelperAction,
    row_action: RowAction,
    name: Arc<str>,
}

/// The confirmation for quitting, force quitting and pausing, and the background runs behind
/// it. Harmless actions (resume, renice) run without asking.
#[derive(Clone, Copy)]
pub struct ActionPrompt {
    state: AppState,
    toasts: Toasts,
    open: Signal<bool>,
    confirmation: Confirmation,
    pending: Store<Option<(ActionTarget, RowAction)>>,
    /// Bumped per request, so a late assessment of an older request is ignored.
    generation: Store<u64>,
    escalated: Store<Vec<Escalated>>,
}

impl ActionPrompt {
    pub fn new(cx: &mut Cx, state: AppState, toasts: Toasts) -> Self {
        let prompt = Self {
            state,
            toasts,
            open: cx.signal(false),
            confirmation: Confirmation::new(cx, "Quit", Some("Force Quit")),
            pending: cx.store(None),
            generation: cx.store(0),
            escalated: cx.store(Vec::new()),
        };
        if let Some(outcomes) = EngineLink::find(cx).and_then(|link| link.helper_outcomes()) {
            follow_broadcast(cx, outcomes, move |cx, outcome: HelperOutcome| {
                prompt.answered(cx, &outcome);
            });
        }
        prompt
    }

    /// Toasts the helper's answer to the last outstanding action of one request.
    fn answered(self, cx: &mut Cx, outcome: &HelperOutcome) {
        let finished = self.escalated.update(cx, |escalated| {
            let index = escalated
                .iter()
                .position(|pending| pending.action == outcome.action)?;
            let done = escalated.swap_remove(index);
            let last = !escalated.iter().any(|pending| pending.name == done.name);
            last.then_some(done)
        });
        if let Some(done) = finished {
            let message = helper_message(&done.name, done.row_action, outcome);
            self.toasts.show(cx, message);
        }
    }

    fn escalate(
        self,
        cx: &mut Cx,
        target: &ActionTarget,
        row_action: RowAction,
        actions: Vec<HelperAction>,
    ) {
        let Some(link) = EngineLink::find(cx) else {
            return;
        };
        let name: Arc<str> = target.name().into();
        self.escalated.update(cx, |escalated| {
            escalated.extend(actions.iter().map(|action| Escalated {
                action: action.clone(),
                row_action,
                name: Arc::clone(&name),
            }));
        });
        self.toasts.show(
            cx,
            format!(
                "{name}: asking the helper to {}",
                row_action.verb().to_lowercase()
            ),
        );
        for action in actions {
            link.send(command(action));
        }
    }

    pub fn request(self, cx: &mut Cx, row: RowId, action: RowAction) {
        let target = self
            .state
            .live
            .with_untracked(cx, |live| ActionTarget::resolve(&live.apps, row));
        let Some(target) = target else {
            self.toasts.show(cx, GONE_MESSAGE);
            return;
        };
        match action {
            RowAction::Signal(ProcessAction::Resume)
            | RowAction::Renice(_)
            | RowAction::Affinity(_) => {
                self.execute(cx, target, action);
            }
            RowAction::Signal(signal) => self.ask(cx, target, action, signal),
        }
    }

    pub fn view(self) -> impl View {
        confirm_dialog(self.open, self.confirmation, move |cx: &mut Cx, choice| {
            let Some((target, action)) = self.pending.update(cx, Option::take) else {
                return;
            };
            let action = match choice {
                ConfirmChoice::Confirm => action,
                ConfirmChoice::Secondary => RowAction::Signal(ProcessAction::ForceQuit),
            };
            self.execute(cx, target, action);
        })
    }

    fn ask(self, cx: &mut Cx, target: ActionTarget, action: RowAction, signal: ProcessAction) {
        let listed = match &target {
            ActionTarget::App { listed, .. } => *listed,
            ActionTarget::Process { .. } => 1,
        };
        let confirmation = self.confirmation;
        confirmation
            .title
            .set(cx, format!("{} {}?", action.verb(), target.name()));
        confirmation.message.set(cx, consequence(signal, listed));
        confirmation.confirm_label.set(cx, action.verb());
        confirmation
            .secondary_label
            .set(cx, (signal == ProcessAction::Quit).then_some("Force Quit"));
        confirmation.allowed.set(cx, true);
        let generation = self.generation.update(cx, |generation| {
            *generation += 1;
            *generation
        });
        self.pending
            .update(cx, |pending| *pending = Some((target.clone(), action)));
        self.open.set(cx, true);

        let snapshot = self.snapshot(cx);
        let checked = async move {
            tokio::task::spawn_blocking(move || assess(&snapshot, &target, signal))
                .await
                .ok()
        };
        cx.spawn(checked, move |cx, assessment| {
            let current = self.generation.with(cx, |current| *current == generation);
            if let (true, Some(assessment)) = (current, assessment) {
                self.show_assessment(cx, signal, assessment);
            }
        });
    }

    fn show_assessment(self, cx: &mut Cx, signal: ProcessAction, assessment: Assessment) {
        let confirmation = self.confirmation;
        match assessment.next {
            NextStep::Refuse(reason) => {
                confirmation.message.set(cx, sentence(reason));
                confirmation.allowed.set(cx, false);
                self.pending.update(cx, |pending| *pending = None);
            }
            NextStep::Confirm | NextStep::Run => {
                confirmation
                    .message
                    .set(cx, consequence(signal, assessment.affected));
            }
        }
    }

    /// Runs the action off the UI thread; a permission error goes to the helper when it can
    /// act for other users.
    fn execute(self, cx: &mut Cx, target: ActionTarget, action: RowAction) {
        let snapshot = self.snapshot(cx);
        let done = async move {
            tokio::task::spawn_blocking(move || {
                let result = run(&snapshot, &target, action);
                let retry = helper_actions(&snapshot, &target, action, &result);
                (outcome_message(&target, action, &result), retry, target)
            })
            .await
            .map_err(|error| error.to_string())
        };
        cx.spawn(done, move |cx, answer| match answer {
            Ok((_, retry, target)) if !retry.is_empty() => {
                self.escalate(cx, &target, action, retry);
            }
            Ok((message, ..)) | Err(message) => self.toasts.show(cx, message),
        });
    }

    fn snapshot(self, cx: &mut Cx) -> Arc<Snapshot> {
        self.state
            .live
            .with_untracked(cx, |live| Arc::clone(&live.tick))
    }
}

/// "96 processes will close."
pub fn consequence(signal: ProcessAction, count: usize) -> String {
    let processes = process_count(count);
    match signal {
        ProcessAction::Quit => format!("{processes} will close."),
        ProcessAction::ForceQuit => {
            format!("{processes} will stop at once; unsaved work is lost.")
        }
        ProcessAction::Pause => format!("{processes} will pause until resumed."),
        ProcessAction::Resume => format!("{processes} will resume."),
    }
}

/// A refusal explanation as a sentence: capitalised, with a full stop.
fn sentence(reason: &str) -> String {
    let mut characters = reason.chars();
    characters.next().map_or_else(String::new, |first| {
        format!("{}{}.", first.to_uppercase(), characters.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consequences_state_the_count() {
        assert_eq!(
            consequence(ProcessAction::Quit, 96),
            "96 processes will close."
        );
        assert_eq!(consequence(ProcessAction::Quit, 1), "1 process will close.");
    }

    #[test]
    fn refusals_read_as_sentences() {
        assert_eq!(sentence("this is the monitor"), "This is the monitor.");
    }
}
