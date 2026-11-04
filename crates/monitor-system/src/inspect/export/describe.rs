use std::borrow::Cow;

use crate::inspect::security::{CapabilitySet, Emulation, Sandbox};

pub(super) fn capabilities(set: &CapabilitySet) -> Cow<'static, str> {
    if set.is_full() {
        Cow::Borrowed("all")
    } else if set.names.is_empty() {
        Cow::Borrowed("none")
    } else {
        Cow::Owned(set.names.join(", "))
    }
}

pub(super) fn sandbox(sandbox: &Sandbox) -> Cow<'static, str> {
    match sandbox {
        Sandbox::None => Cow::Borrowed("none"),
        Sandbox::Flatpak { app_id } => match app_id
            .as_deref()
            .map(str::trim_end)
            .filter(|app_id| !app_id.is_empty())
        {
            Some(app_id) => Cow::Owned(format!("Flatpak {app_id}")),
            None => Cow::Borrowed("Flatpak"),
        },
        Sandbox::Snap { name } => Cow::Owned(format!("Snap {name}")),
        Sandbox::Bubblewrap => Cow::Borrowed("bubblewrap"),
        Sandbox::Firejail => Cow::Borrowed("firejail"),
        Sandbox::Container { id } => Cow::Owned(format!("container {id}")),
    }
}

pub(super) fn emulation(emulation: &Emulation) -> Cow<'static, str> {
    match emulation {
        Emulation::Wine => Cow::Borrowed("Wine"),
        Emulation::Proton => Cow::Borrowed("Proton"),
        Emulation::Box64 => Cow::Borrowed("box64"),
        Emulation::Fex => Cow::Borrowed("FEX"),
        Emulation::Binfmt {
            handler,
            interpreter,
        } => Cow::Owned(format!("binfmt {handler} ({interpreter})")),
    }
}
