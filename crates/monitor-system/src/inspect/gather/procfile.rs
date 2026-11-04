use std::io;

use monitor_core::{Coverage, Pid, SourceError};

/// Why a per-process file could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReadFailure {
    Denied,
    Unavailable,
}

impl From<io::Error> for ReadFailure {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::PermissionDenied {
            Self::Denied
        } else {
            Self::Unavailable
        }
    }
}

pub(super) fn process_file(pid: Pid, name: &str) -> String {
    format!("/proc/{pid}/{name}")
}

pub(super) fn gone(pid: Pid) -> SourceError {
    SourceError::io(
        process_file(pid, "stat"),
        io::Error::from(io::ErrorKind::NotFound),
    )
}

pub(super) fn read_text(pid: Pid, name: &str) -> Result<String, ReadFailure> {
    Ok(String::from_utf8_lossy(&std::fs::read(process_file(pid, name))?).into_owned())
}

pub(super) fn read_link(pid: Pid, name: &str) -> Result<String, ReadFailure> {
    Ok(std::fs::read_link(process_file(pid, name))?
        .to_string_lossy()
        .into_owned())
}

/// Tracks whether any restricted read was refused.
#[derive(Default)]
pub(super) struct Restrictions {
    denied: bool,
}

impl Restrictions {
    pub(super) fn note<T>(&mut self, result: Result<T, ReadFailure>) -> Option<T> {
        result
            .map_err(|failure| self.denied |= failure == ReadFailure::Denied)
            .ok()
    }

    pub(super) fn coverage(&self) -> Coverage {
        if self.denied {
            Coverage::OwnProcesses
        } else {
            Coverage::Full
        }
    }
}
