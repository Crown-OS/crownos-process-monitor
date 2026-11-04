use super::verdict::RefuseReason;

#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error("refused: {}", .0.explanation())]
    Refused(RefuseReason),
    #[error("choose at least one CPU")]
    NoCpus,
    #[error("the process has already exited")]
    Gone,
    /// Another user's process; the privileged helper can do it.
    #[error("permission denied")]
    PermissionDenied,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl From<rustix::io::Errno> for ActionError {
    fn from(errno: rustix::io::Errno) -> Self {
        match errno {
            rustix::io::Errno::SRCH | rustix::io::Errno::NOENT => Self::Gone,
            rustix::io::Errno::PERM | rustix::io::Errno::ACCESS => Self::PermissionDenied,
            other => Self::Io(other.into()),
        }
    }
}
