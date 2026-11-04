mod affinity;
mod app;
mod cpu_mask;
mod error;
mod signal;
mod verdict;

pub use affinity::{affinity, set_affinity};
pub use app::{app_verdict, perform_app, processes_affected};
pub use cpu_mask::CpuMask;
pub use error::ActionError;
pub use signal::{perform, renice};
pub use verdict::{RefuseReason, Verdict, verdict};

/// What the user asked to do to a process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProcessAction {
    /// `SIGTERM`: ask it to exit.
    Quit,
    /// `SIGKILL`.
    ForceQuit,
    /// `SIGSTOP`, or freezing the app's cgroup.
    Pause,
    /// `SIGCONT`, or thawing the app's cgroup.
    Resume,
}

/// The same actions applied to every process of an app.
pub type AppAction = ProcessAction;
