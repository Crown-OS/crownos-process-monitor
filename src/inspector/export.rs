use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use crownui::kit::{ButtonOptions, ButtonVariant, Toasts, button, icons};
use crownui::prelude::{Cx, Listeners, Signal, Styled, View, hstack};
use monitor_engine::system::inspect::{EnvironmentExport, ProcessDetails};

use super::load::InspectorData;
use crate::util::downloads::{download_dir, export_file_name};
use crate::util::spacing;

fn current(cx: &mut Cx, data: Signal<InspectorData>) -> Option<Arc<ProcessDetails>> {
    data.with_untracked(cx, |data| data.details().cloned())
}

/// Puts the details on the clipboard as text; the environment stays out.
pub fn copy_as_text(cx: &mut Cx, data: Signal<InspectorData>, toasts: Toasts) {
    if let Some(details) = current(cx, data) {
        cx.copy_to_clipboard(details.to_text(EnvironmentExport::Exclude));
        toasts.show(cx, "Copied the process details");
    }
}

fn save_csv(details: &ProcessDetails) -> Result<PathBuf> {
    let folder = download_dir().context("no download folder: set XDG_DOWNLOAD_DIR")?;
    std::fs::create_dir_all(&folder)
        .with_context(|| format!("cannot create {}", folder.display()))?;
    let identity = &details.identity;
    let path = folder.join(export_file_name(
        &identity.name,
        identity.key.pid.get(),
        "csv",
    ));
    std::fs::write(&path, details.to_csv(EnvironmentExport::Exclude))
        .with_context(|| format!("cannot write {}", path.display()))?;
    Ok(path)
}

/// Writes the details as CSV into the download folder on a blocking task.
pub fn save_as_csv(cx: &mut Cx, data: Signal<InspectorData>, toasts: Toasts) {
    let Some(details) = current(cx, data) else {
        return;
    };
    let saving = async move {
        tokio::task::spawn_blocking(move || save_csv(&details))
            .await
            .map_err(anyhow::Error::from)
            .and_then(|saved| saved)
    };
    cx.spawn(saving, move |cx, saved| {
        let message = match saved {
            Ok(path) => format!("Saved {}", path.display()),
            Err(error) => format!("Could not save: {error:#}"),
        };
        toasts.show(cx, message);
    });
}

pub fn export_buttons(data: Signal<InspectorData>, toasts: Toasts) -> impl View {
    hstack((
        button("Copy as Text")
            .leading_icon(icons::COPY)
            .variant(ButtonVariant::Secondary)
            .on_click(move |cx, _| copy_as_text(cx, data, toasts)),
        button("Save as CSV")
            .leading_icon(icons::DOWNLOAD)
            .variant(ButtonVariant::Secondary)
            .on_click(move |cx, _| save_as_csv(cx, data, toasts)),
    ))
    .gap(spacing::COMFORTABLE)
}
