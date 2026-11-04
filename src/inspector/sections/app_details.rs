use crownui::prelude::{AnyView, Signal};
use monitor_engine::system::inspect::ProcessDetails;

use super::folded::folded_text;
use crate::inspector::fields::{Fields, section};

/// The `.desktop` entry the app was matched to, with the whole file folded below.
pub fn view(details: &ProcessDetails, file_open: Signal<bool>) -> Option<AnyView> {
    let entry = details.desktop_entry.as_ref()?;
    let mut fields = Fields::default();
    fields
        .add_some("Name", entry.name.clone())
        .add_some("Generic name", entry.generic_name.clone())
        .add("Desktop id", entry.id.clone())
        .add_some("Icon", entry.icon.clone())
        .add_some("Categories", entry.categories.clone())
        .add_some("Exec", entry.exec.clone())
        .add_some("StartupWMClass", entry.startup_wm_class.clone())
        .add("File", entry.path.display().to_string());
    let mut rows = fields.into_views();
    if !entry.text.is_empty() {
        rows.push(folded_text("file", file_open, entry.text.clone()));
    }
    Some(section("App details", rows))
}
