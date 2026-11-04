use monitor_system::ActionError;

/// The errors the helper replies with, named
/// `org.crownos.ProcessMonitor1.Error.<Variant>` on the bus. Proxies decode
/// them back into the same variants.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.crownos.ProcessMonitor1.Error")]
pub enum HelperError {
    /// Transport failure, or the helper is not installed.
    #[zbus(error)]
    ZBus(zbus::Error),
    NotAuthorized(String),
    /// Allowed after the person authenticates, which did not happen.
    NeedsAuthentication(String),
    /// This build or this machine cannot do it.
    NotSupported(String),
    InvalidArgument(String),
    /// The process is on the "leave it alone" list.
    Refused(String),
    /// The process already exited, or its PID now names another process.
    Gone(String),
    Failed(String),
}

impl HelperError {
    pub fn invalid(what: impl Into<String>) -> Self {
        Self::InvalidArgument(what.into())
    }

    pub fn failed(error: impl std::fmt::Display) -> Self {
        Self::Failed(error.to_string())
    }

    /// The caller lacks the authorization, as opposed to a failure.
    pub const fn is_denial(&self) -> bool {
        matches!(self, Self::NotAuthorized(_) | Self::NeedsAuthentication(_))
    }
}

impl From<ActionError> for HelperError {
    fn from(error: ActionError) -> Self {
        match error {
            ActionError::Refused(reason) => Self::Refused(reason.explanation().to_owned()),
            ActionError::Gone => Self::Gone(error.to_string()),
            ActionError::PermissionDenied | ActionError::NoCpus | ActionError::Io(_) => {
                Self::failed(error)
            }
        }
    }
}
