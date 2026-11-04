use monitor_core::{AppKind, ProcessKey};

use super::verdict::{RefuseReason, Verdict, verdict};
use super::{ActionError, AppAction, ProcessAction, perform};
use crate::apps::scope::CGROUP_ROOT;
use crate::process::ProcessTable;

fn members(table: &ProcessTable, app: usize) -> impl Iterator<Item = ProcessKey> + '_ {
    table.processes_of(app).map(|row| row.key)
}

/// The verdict for an app: refused when any member is, otherwise like a
/// single process.
pub fn app_verdict(table: &ProcessTable, app: usize, action: AppAction) -> Verdict {
    let Some(row) = table.apps.get(app) else {
        return Verdict::AskFirst;
    };
    if row.kind == AppKind::Kernel {
        return Verdict::Refuse(RefuseReason::KernelThread);
    }
    members(table, app)
        .map(|key| verdict(key, action))
        .find(|verdict| matches!(verdict, Verdict::Refuse(_)))
        .unwrap_or(if action == ProcessAction::Resume {
            Verdict::Safe
        } else {
            Verdict::AskFirst
        })
}

/// How many processes the action reaches, for the confirmation text. An
/// app scope counts every process in it, including ones not yet sampled.
pub fn processes_affected(table: &ProcessTable, app: usize) -> usize {
    let scope_count = table
        .apps
        .get(app)
        .and_then(|row| row.scope.as_ref())
        .and_then(|scope| {
            std::fs::read_to_string(format!("{CGROUP_ROOT}{scope}/cgroup.procs")).ok()
        })
        .map(|procs| procs.lines().filter(|line| !line.is_empty()).count());
    scope_count.unwrap_or_else(|| members(table, app).count())
}

/// Applies `action` to a whole app. With its own cgroup scope, force quit
/// writes `cgroup.kill` and pause/resume toggle `cgroup.freeze`; otherwise
/// each member is signalled through a pidfd. Returns the processes reached.
pub fn perform_app(
    table: &ProcessTable,
    app: usize,
    action: AppAction,
) -> Result<usize, ActionError> {
    if let Verdict::Refuse(reason) = app_verdict(table, app, action) {
        return Err(ActionError::Refused(reason));
    }
    let affected = processes_affected(table, app);
    if let Some(scope) = table.apps.get(app).and_then(|row| row.scope.as_ref()) {
        let control = match action {
            ProcessAction::ForceQuit => Some(("cgroup.kill", "1")),
            ProcessAction::Pause => Some(("cgroup.freeze", "1")),
            ProcessAction::Resume => Some(("cgroup.freeze", "0")),
            ProcessAction::Quit => None,
        };
        if let Some((file, value)) = control
            && std::fs::write(format!("{CGROUP_ROOT}{scope}/{file}"), value).is_ok()
        {
            return Ok(affected);
        }
    }
    signal_members(table, app, action)
}

fn signal_members(
    table: &ProcessTable,
    app: usize,
    action: ProcessAction,
) -> Result<usize, ActionError> {
    let mut reached = 0;
    let mut denied = false;
    for key in members(table, app) {
        match perform(key, action) {
            Ok(()) => reached += 1,
            Err(ActionError::Gone) => {}
            Err(ActionError::PermissionDenied) => denied = true,
            Err(error) => return Err(error),
        }
    }
    if reached == 0 && denied {
        Err(ActionError::PermissionDenied)
    } else {
        Ok(reached)
    }
}
