use std::path::PathBuf;
use std::sync::Arc;

use super::fields::{DesktopFields, exec_parts};
use crate::util::text::basename;

/// Launchers whose name says nothing about the app they start.
pub(super) const GENERIC_LAUNCHERS: [&str; 12] = [
    "env",
    "sh",
    "bash",
    "flatpak",
    "python",
    "python3",
    "java",
    "sudo",
    "pkexec",
    "gtk-launch",
    "xdg-open",
    "snap",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DesktopEntry {
    pub id: Arc<str>,
    pub name: Arc<str>,
    pub icon: Option<Arc<str>>,
    pub path: PathBuf,
    pub no_display: bool,
    /// `Exec=` passes arguments, so it starts something more specific than
    /// the bare program (`kitty /path/script.sh`).
    launches_with_arguments: bool,
    pub(super) wm_class: Option<String>,
    pub(super) program: Option<String>,
    pub(super) try_exec: Option<String>,
}

impl DesktopEntry {
    pub(super) fn from_fields(id: &str, path: PathBuf, fields: DesktopFields) -> Option<Self> {
        if fields.hidden
            || fields
                .kind
                .as_deref()
                .is_some_and(|kind| kind != "Application")
        {
            return None;
        }
        let exec = fields.exec.as_deref().and_then(exec_parts);
        let program = exec
            .map(|(program, _)| program)
            .filter(|program| !GENERIC_LAUNCHERS.contains(program))
            .map(str::to_owned);
        Some(Self {
            id: id.into(),
            name: fields.name.as_deref().unwrap_or(id).into(),
            icon: fields.icon.map(Into::into),
            path,
            no_display: fields.no_display,
            launches_with_arguments: exec.is_some_and(|(_, has_arguments)| has_arguments),
            wm_class: fields.startup_wm_class,
            program,
            try_exec: fields.try_exec.as_deref().map(basename).map(str::to_owned),
        })
    }

    /// Lower is a better match for a bare program name.
    pub(super) fn rank(&self) -> (bool, bool) {
        (self.no_display, self.launches_with_arguments)
    }
}
