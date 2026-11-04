use std::sync::Arc;

/// Failures of user-requested UDisks2 actions (unmount, eject, power off).
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("D-Bus request failed: {0}")]
    Bus(#[from] zbus::Error),
    #[error("{0} is not known to UDisks2")]
    UnknownDevice(Arc<str>),
    #[error("UDisks2 is not available")]
    Unavailable,
    #[error("the UDisks2 worker has stopped")]
    WorkerStopped,
}
