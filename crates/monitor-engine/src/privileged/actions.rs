use std::sync::Arc;

use monitor_core::ProcessKey;
use monitor_hardware::FanId;
use monitor_helper::{ActionReceiver, HelperClient, HelperError, SignalKind, fan_id_string};
use tokio::sync::broadcast;

use crate::util::reply::forward;

const THREAD_NAME: &str = "monitor-helper-action";
/// Outcomes a slow subscriber may lag behind before it misses some.
pub(crate) const OUTCOME_BACKLOG: usize = 16;

/// A write the privileged helper performs, after authorization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelperAction {
    Signal {
        key: ProcessKey,
        signal: SignalKind,
    },
    Renice {
        key: ProcessKey,
        nice: i32,
    },
    /// `percent` of full PWM duty; the helper restores automatic control
    /// when the engine disconnects.
    SetFan {
        fan: FanId,
        percent: u8,
    },
    RestoreFans,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelperFailure {
    /// No helper is installed or it is unreachable.
    Unavailable,
    /// The authorization backend refused, or authentication was dismissed.
    Denied(Arc<str>),
    /// Refused, gone, unsupported or failed, with the helper's reason.
    Failed(Arc<str>),
}

impl From<HelperError> for HelperFailure {
    fn from(error: HelperError) -> Self {
        match error {
            HelperError::ZBus(_) => Self::Unavailable,
            denial if denial.is_denial() => Self::Denied(denial.to_string().into()),
            other => Self::Failed(other.to_string().into()),
        }
    }
}

/// What became of one [`HelperAction`]; see `EngineControl::helper_outcomes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelperOutcome {
    pub action: HelperAction,
    pub result: Result<(), HelperFailure>,
}

/// Sends `action` and reports its outcome once the helper answered, which
/// may take as long as an authentication dialog. A short-lived thread
/// waits for the answer, so the sampler never does.
pub(crate) fn perform(
    client: Option<&HelperClient>,
    action: HelperAction,
    outcomes: &broadcast::Sender<HelperOutcome>,
) {
    let Some(client) = client else {
        let _ = outcomes.send(HelperOutcome {
            action,
            result: Err(HelperFailure::Unavailable),
        });
        return;
    };
    let answer = request(client, &action);
    forward(THREAD_NAME, answer, outcomes, move |answer| HelperOutcome {
        action,
        result: match answer {
            Some(result) => result.map_err(HelperFailure::from),
            None => Err(HelperFailure::Unavailable),
        },
    });
}

fn request(client: &HelperClient, action: &HelperAction) -> ActionReceiver {
    match action {
        HelperAction::Signal { key, signal } => client.signal(*key, *signal),
        HelperAction::Renice { key, nice } => client.renice(*key, *nice),
        HelperAction::SetFan { fan, percent } => client.set_fan(fan_id_string(fan), *percent),
        HelperAction::RestoreFans => client.restore_fans(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_a_missing_helper_at_once() {
        let (outcomes, mut received) = broadcast::channel(OUTCOME_BACKLOG);
        perform(None, HelperAction::RestoreFans, &outcomes);
        assert_eq!(
            received.try_recv().ok(),
            Some(HelperOutcome {
                action: HelperAction::RestoreFans,
                result: Err(HelperFailure::Unavailable),
            })
        );
    }

    #[test]
    fn classifies_helper_errors() {
        assert!(matches!(
            HelperFailure::from(HelperError::NotAuthorized("no".into())),
            HelperFailure::Denied(_)
        ));
        assert!(matches!(
            HelperFailure::from(HelperError::Gone("exited".into())),
            HelperFailure::Failed(_)
        ));
    }
}
