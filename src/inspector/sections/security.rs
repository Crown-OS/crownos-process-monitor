use crownui::prelude::AnyView;
use monitor_engine::system::inspect::{CapabilitySet, ProcessDetails, Sandbox, SeccompMode};

use crate::inspector::fields::{Fields, field_section, yes_no};

pub fn sandbox_name(sandbox: &Sandbox) -> String {
    match sandbox {
        Sandbox::None => "None".to_owned(),
        Sandbox::Flatpak { app_id: Some(id) } => format!("Flatpak · {id}"),
        Sandbox::Flatpak { app_id: None } => "Flatpak".to_owned(),
        Sandbox::Snap { name } => format!("Snap · {name}"),
        Sandbox::Bubblewrap => "bubblewrap".to_owned(),
        Sandbox::Firejail => "Firejail".to_owned(),
        Sandbox::Container { id } => format!("Container · {id}"),
    }
}

const fn seccomp_name(mode: SeccompMode) -> &'static str {
    match mode {
        SeccompMode::Disabled => "Off",
        SeccompMode::Strict => "Strict",
        SeccompMode::Filter => "Filter",
    }
}

fn capabilities(set: &CapabilitySet) -> String {
    if set.is_full() {
        "All".to_owned()
    } else if set.names.is_empty() {
        "None".to_owned()
    } else {
        set.names.join(", ")
    }
}

pub fn view(details: &ProcessDetails) -> Option<AnyView> {
    let security = &details.security;
    let mut fields = Fields::default();
    fields
        .add("Sandbox", sandbox_name(&security.sandbox))
        .add("Own namespaces", security.namespaces.join(", "))
        .add("Seccomp", seccomp_name(security.seccomp))
        .add("No new privileges", yes_no(security.no_new_privileges))
        .add("Effective caps", capabilities(&security.effective))
        .add("Permitted caps", capabilities(&security.permitted))
        .add("Bounding caps", capabilities(&security.bounding))
        .add_some("LSM label", security.lsm_label.clone());
    field_section("Security", fields)
}
