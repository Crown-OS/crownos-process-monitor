//! ACPI platform profile (`low-power`, `balanced`, `performance`…).

use std::path::Path;
use std::sync::Arc;

use super::snapshot::PlatformProfile;
use crate::util::{attribute, directory};

pub(crate) fn read(sysfs: &Path) -> Option<PlatformProfile> {
    let legacy = sysfs.join("firmware/acpi");
    read_pair(&legacy, "platform_profile", "platform_profile_choices").or_else(|| {
        let class = sysfs.join("class/platform-profile");
        directory::entries(&class, |_| true)
            .into_iter()
            .find_map(|handler| read_pair(&class.join(handler), "profile", "choices"))
    })
}

fn read_pair(directory: &Path, active: &str, choices: &str) -> Option<PlatformProfile> {
    let active = attribute::read_text(directory.join(active))?;
    let choices = attribute::read_text(directory.join(choices))
        .map(|choices| choices.split_whitespace().map(Arc::from).collect())
        .unwrap_or_default();
    Some(PlatformProfile { active, choices })
}
