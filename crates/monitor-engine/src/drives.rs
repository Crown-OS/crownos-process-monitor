use std::sync::Arc;

use monitor_io::UDisks;
use tokio::sync::broadcast;

use crate::util::reply::forward;

const THREAD_NAME: &str = "monitor-drive-action";
/// Outcomes a slow subscriber may lag behind before it misses some.
pub(crate) const DRIVE_OUTCOME_BACKLOG: usize = 8;
const NO_UDISKS: &str = "UDisks2 is not available";

/// What UDisks2 does to a volume or drive, after polkit allows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DriveAction {
    /// Unmounts the filesystem on the block device.
    Unmount,
    /// Ejects the medium of the drive behind it (optical, SD).
    Eject,
    /// Powers the drive off so a USB disk can be unplugged; unmount first.
    PowerOff,
}

/// What became of one [`crate::Command::Drive`]; see `EngineControl::drive_outcomes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriveOutcome {
    pub block_device: Arc<str>,
    pub action: DriveAction,
    /// UDisks2's or polkit's reason on failure.
    pub result: Result<(), Arc<str>>,
}

pub(crate) fn perform(
    udisks: Option<&UDisks>,
    block_device: Arc<str>,
    action: DriveAction,
    outcomes: &broadcast::Sender<DriveOutcome>,
) {
    let Some(udisks) = udisks else {
        let _ = outcomes.send(DriveOutcome {
            block_device,
            action,
            result: Err(NO_UDISKS.into()),
        });
        return;
    };
    let reply = match action {
        DriveAction::Unmount => udisks.unmount(&block_device),
        DriveAction::Eject => udisks.eject(&block_device),
        DriveAction::PowerOff => udisks.power_off(&block_device),
    };
    forward(THREAD_NAME, reply, outcomes, move |answer| DriveOutcome {
        block_device,
        action,
        result: match answer {
            Some(Ok(())) => Ok(()),
            Some(Err(error)) => Err(error.to_string().into()),
            None => Err(NO_UDISKS.into()),
        },
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_missing_udisks_at_once() {
        let (outcomes, mut received) = broadcast::channel(DRIVE_OUTCOME_BACKLOG);
        perform(None, "sdb1".into(), DriveAction::Unmount, &outcomes);
        assert_eq!(
            received.try_recv().ok().map(|outcome| outcome.result),
            Some(Err(NO_UDISKS.into()))
        );
    }
}
