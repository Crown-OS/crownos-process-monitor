use crownui::prelude::AnyView;
use monitor_engine::system::inspect::ProcessDetails;

use crate::inspector::fields::{Fields, field_section};

pub fn view(details: &ProcessDetails) -> Option<AnyView> {
    let startup = &details.startup;
    let mut fields = Fields::default();
    fields
        .add_some("systemd unit", startup.unit.clone())
        .add_some("Description", startup.description.clone())
        .add_some(
            "Unit file",
            startup
                .unit_file
                .as_ref()
                .map(|path| path.display().to_string()),
        )
        .add_some(
            "Autostart",
            startup
                .autostart
                .as_ref()
                .map(|path| path.display().to_string()),
        );
    field_section("Startup", fields)
}
