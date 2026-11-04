use monitor_engine::Snapshot;
use monitor_engine::system::{ProcessAction, Verdict, app_verdict, processes_affected, verdict};

use super::target::ActionTarget;

/// What happens after the user picks an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextStep {
    /// Harmless (resuming): run at once.
    Run,
    /// Show the confirmation with the process count.
    Confirm,
    /// Never allowed; the dialog explains why.
    Refuse(&'static str),
}

impl From<Verdict> for NextStep {
    fn from(verdict: Verdict) -> Self {
        match verdict {
            Verdict::Safe => Self::Run,
            Verdict::AskFirst => Self::Confirm,
            Verdict::Refuse(reason) => Self::Refuse(reason.explanation()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assessment {
    pub next: NextStep,
    pub affected: usize,
}

/// Reads `/proc` and the app's cgroup: run it on a blocking task.
pub fn assess(snapshot: &Snapshot, target: &ActionTarget, action: ProcessAction) -> Assessment {
    match target {
        ActionTarget::Process { key, .. } => Assessment {
            next: verdict(*key, action).into(),
            affected: 1,
        },
        ActionTarget::App { key, listed, .. } => {
            let table = &snapshot.processes;
            match table.app_index(key) {
                Some(app) => Assessment {
                    next: app_verdict(table, app, action).into(),
                    affected: processes_affected(table, app),
                },
                None => Assessment {
                    next: NextStep::Confirm,
                    affected: *listed,
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use monitor_engine::system::RefuseReason;

    use super::*;

    #[test]
    fn verdicts_map_to_the_next_step() {
        assert_eq!(NextStep::from(Verdict::Safe), NextStep::Run);
        assert_eq!(NextStep::from(Verdict::AskFirst), NextStep::Confirm);
        assert_eq!(
            NextStep::from(Verdict::Refuse(RefuseReason::Init)),
            NextStep::Refuse(RefuseReason::Init.explanation())
        );
    }
}
