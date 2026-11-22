//! Captures every page, table state, menu, dialog and drawer of the Process Monitor for the
//! CrownOS-Design Figma file, over a deterministic laptop instead of the live machine. Run both
//! appearances with `../crownui/capture/scripts/capture-app.sh .`; output lands in
//! `$CROWN_DESIGN_DIR`.

mod apps;
mod devices;
mod history;
mod inspect;
mod machine;
mod states;
mod tasks;
mod wave;
mod window;

use anyhow::Result;
use capture::Carrier;

use crate::window::Fixture;

const PAGE_DESCRIPTION: &str = "crownos-process-monitor: every page, table state, row menu, confirmation, the inspector drawer and the settings dialog, captured headlessly from the current source over a deterministic sample laptop.";

fn main() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut carrier = Carrier::new("Process Monitor", PAGE_DESCRIPTION);
    let only = std::env::var("DESIGN_ONLY").unwrap_or_default();
    let groups: [(&str, states::Group); 7] = [
        ("pages", states::pages),
        ("full", states::full_pages),
        ("table", states::table),
        ("menus", states::menus),
        ("dialogs", states::dialogs),
        ("inspector", states::inspector),
        ("devices", states::history_and_devices),
    ];
    for (name, capture) in groups {
        if only.is_empty() || only.split(',').any(|wanted| wanted == name) {
            capture(&fixture, &mut carrier)?;
        }
    }
    let path = carrier.write(None)?;
    println!("{}", path.display());
    capture::finish()
}
