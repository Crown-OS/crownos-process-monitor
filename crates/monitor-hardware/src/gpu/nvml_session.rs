//! NVML library lifetime: opened lazily while an NVIDIA GPU is awake and
//! shut down once every NVIDIA GPU sleeps.

use std::ffi::OsStr;
use std::time::Duration;

use nvml_wrapper::Nvml;
use nvml_wrapper::error::NvmlError;

use monitor_core::util::cadence::Cadence;

const LIBRARY: &str = "libnvidia-ml.so.1";
const RETRY_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionChange {
    Unchanged,
    Opened,
    Closed,
}

#[derive(Default)]
pub(crate) struct NvmlSession {
    nvml: Option<Nvml>,
    library_missing: bool,
    retry: Cadence,
}

impl std::fmt::Debug for NvmlSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NvmlSession")
            .field("open", &self.nvml.is_some())
            .field("library_missing", &self.library_missing)
            .finish()
    }
}

impl NvmlSession {
    pub(crate) fn sync(&mut self, wanted: bool, now: Duration) -> SessionChange {
        match (wanted, self.nvml.is_some()) {
            (false, true) => {
                self.nvml = None;
                self.retry.reset();
                SessionChange::Closed
            }
            (true, false) if self.retry.due(now, RETRY_INTERVAL) => match open() {
                Ok(nvml) => {
                    self.nvml = Some(nvml);
                    self.library_missing = false;
                    SessionChange::Opened
                }
                Err(error) => {
                    self.library_missing = matches!(
                        error,
                        NvmlError::LibloadingError(_) | NvmlError::LibraryNotFound
                    );
                    SessionChange::Unchanged
                }
            },
            _ => SessionChange::Unchanged,
        }
    }

    pub(crate) fn get(&self) -> Option<&Nvml> {
        self.nvml.as_ref()
    }

    pub(crate) const fn is_library_missing(&self) -> bool {
        self.library_missing
    }
}

fn open() -> Result<Nvml, NvmlError> {
    Nvml::builder().lib_path(OsStr::new(LIBRARY)).init()
}
