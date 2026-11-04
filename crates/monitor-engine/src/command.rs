use std::sync::Arc;

use monitor_alerts::{EntryId, QuietScope, QuietUntil};
use monitor_core::{Interest, ProcessKey, SourceResult};
use monitor_hardware::FanId;
use monitor_helper::SignalKind;
use monitor_system::ProcessDetails;
use tokio::sync::oneshot;

use crate::{DriveAction, EngineSettings, Notifications};

/// Requests from the UI, applied on the sampler thread between ticks.
#[derive(Clone, Debug)]
pub enum Command {
    SetSettings(EngineSettings),
    /// Starts or stops desktop popups for new alerts.
    SetNotifications(Notifications),
    Quiet {
        scope: QuietScope,
        until: QuietUntil,
    },
    Unquiet(QuietScope),
    PinAlert {
        entry: EntryId,
        pinned: bool,
    },
    /// Asks UDisks2 for fresh SMART data now instead of in five minutes.
    RefreshDriveHealth,
    /// Unmounts, ejects or powers off through UDisks2 (polkit may ask);
    /// `block_device` is a kernel name such as `sdb1`. The outcome arrives
    /// on `EngineControl::drive_outcomes`.
    Drive {
        block_device: Arc<str>,
        action: DriveAction,
    },
    DeleteHistory,
    /// Signals a process through the privileged helper (other users'
    /// processes; the helper asks for authentication). The outcome arrives
    /// on `EngineControl::helper_outcomes`, like every helper action.
    HelperSignal {
        key: ProcessKey,
        signal: SignalKind,
    },
    /// Changes a process's nice value through the helper, which may raise
    /// priority (negative nice) and touch other users' processes.
    HelperRenice {
        key: ProcessKey,
        nice: i32,
    },
    /// Holds a fan at `percent` duty through the helper; it restores
    /// automatic control when the engine stops.
    SetFan {
        fan: FanId,
        percent: u8,
    },
    RestoreFans,
    Shutdown,
}

pub(crate) enum Message {
    Interest(Interest),
    Command(Command),
    Inspect {
        key: ProcessKey,
        reply: oneshot::Sender<SourceResult<ProcessDetails>>,
    },
}
