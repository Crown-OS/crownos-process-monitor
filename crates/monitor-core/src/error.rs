use std::borrow::Cow;
use std::io;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: Cow<'static, str>,
        #[source]
        source: io::Error,
    },
    #[error("malformed {what}")]
    Parse { what: &'static str },
    #[error("{0} is unavailable")]
    Unavailable(&'static str),
}

impl SourceError {
    pub fn io(path: impl Into<Cow<'static, str>>, source: impl Into<io::Error>) -> Self {
        Self::Io {
            path: path.into(),
            source: source.into(),
        }
    }

    /// `path` vanished: the process exited or the device was unplugged.
    pub fn gone(path: impl Into<Cow<'static, str>>) -> Self {
        Self::io(path, io::Error::from(io::ErrorKind::NotFound))
    }

    #[must_use]
    pub const fn parse(what: &'static str) -> Self {
        Self::Parse { what }
    }

    /// The process or device went away between enumeration and reading.
    #[must_use]
    pub fn is_gone(&self) -> bool {
        match self {
            Self::Io { source, .. } => {
                source.kind() == io::ErrorKind::NotFound
                    || source.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error())
            }
            Self::Parse { .. } | Self::Unavailable(_) => false,
        }
    }

    #[must_use]
    pub fn is_permission_denied(&self) -> bool {
        matches!(self, Self::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied)
    }
}

pub type SourceResult<T> = Result<T, SourceError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gone_is_gone() {
        assert!(SourceError::gone("/proc/1/stat").is_gone());
        assert!(!SourceError::parse("stat").is_gone());
    }
}
