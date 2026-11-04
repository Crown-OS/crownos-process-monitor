use std::sync::Arc;

use crownui::kit::{StatusOptions, ToggleOptions, Tone, icons, status, toggle, use_theme};
use crownui::prelude::{
    AnyView, Color, Memo, Role, Semantic, Signal, Styled, View, bind, component, hstack, keyed,
    spacer, text, vstack,
};
use monitor_core::Coverage;
use monitor_engine::DriveAction;
use monitor_engine::io::{Volume, VolumeLocation, VolumeRole};

use super::drive_actions::DriveActions;
use crate::app::AppState;
use crate::components::capacity_bar::{Capacity, capacity_bar};
use crate::components::empty_state::empty_state;
use crate::util::spacing;

const NAME_SIZE: f32 = 13.0;
const DETAIL_SIZE: f32 = 12.0;

/// The mount point users know a volume by: `/`, `/home`, `/run/media/…`.
fn title(volume: &Volume) -> String {
    let mount = volume
        .primary_mount_point()
        .map_or_else(|| volume.source.to_string(), |mount| mount.path.to_string());
    match &volume.label {
        Some(label) if **label != *mount => format!("{label} · {mount}"),
        _ => mount,
    }
}

fn details(volume: &Volume) -> String {
    let device = volume.block_device.as_deref().unwrap_or(&volume.source);
    let mut details = format!("{} · {device}", volume.fs_type);
    let others: Vec<&str> = volume
        .mount_points
        .iter()
        .skip(1)
        .map(|mount| &*mount.path)
        .collect();
    if !others.is_empty() {
        details.push_str(" · also at ");
        details.push_str(&others.join(", "));
    }
    details
}

fn badges(volume: &Volume, drives: DriveActions) -> Vec<AnyView> {
    let mut badges = Vec::new();
    if volume.read_only {
        badges.push(status("Read-only").tone(Tone::Warning).boxed());
    }
    if volume.location == VolumeLocation::Remote {
        badges.push(status("Network").tone(Tone::Neutral).boxed());
    }
    if volume
        .capacity
        .is_some_and(|capacity| capacity.coverage == Coverage::Estimated)
    {
        badges.push(status("Approximate").tone(Tone::Neutral).boxed());
    }
    if volume.removable {
        badges.push(status("Removable").tone(Tone::Neutral).boxed());
        if let Some(block_device) = &volume.block_device {
            let name: Arc<str> = title(volume).into();
            badges.extend(
                [DriveAction::Unmount, DriveAction::Eject].map(|action| {
                    drives.button(Arc::clone(block_device), Arc::clone(&name), action)
                }),
            );
        }
    }
    badges
}

fn volume_row(volume: &Volume, tint: Memo<Color>, drives: DriveActions) -> AnyView {
    let name = title(volume);
    let detail = details(volume);
    let capacity = volume.capacity.map(|capacity| Capacity {
        used: capacity.used.get(),
        total: capacity.size.get(),
    });
    let badges = badges(volume, drives);
    component(move |cx| {
        let theme = use_theme(cx);
        let bar = capacity.map(|capacity| capacity_bar(move |_: &mut _| capacity, tint));
        vstack((
            hstack((
                text(name.clone())
                    .text_size(NAME_SIZE)
                    .font_medium()
                    .truncate(),
                spacer(),
                hstack(badges).gap(spacing::SNUG).items_center(),
            ))
            .gap(spacing::COMFORTABLE)
            .items_center(),
            text(detail)
                .text_size(DETAIL_SIZE)
                .text_color(bind(move |runtime| {
                    theme.with(runtime, |theme| theme.text.muted)
                }))
                .truncate(),
            bar,
        ))
        .gap(spacing::SNUG)
        .w_full()
        .role(Role::ListItem)
        .label(name)
    })
    .boxed()
}

fn volume_list(
    volumes: &[Volume],
    show_system: bool,
    tint: Memo<Color>,
    drives: DriveActions,
) -> AnyView {
    let rows: Vec<AnyView> = volumes
        .iter()
        .filter(|volume| show_system || volume.role == VolumeRole::User)
        .map(|volume| volume_row(volume, tint, drives))
        .collect();
    if rows.is_empty() {
        return empty_state(icons::HARD_DRIVE, "No volumes", "Nothing is mounted.").boxed();
    }
    vstack(rows).gap(spacing::WIDE).w_full().boxed()
}

/// Mounted volumes with capacity bars, removable ones with Unmount and Eject; boot and firmware
/// partitions behind a switch. Rebuilt only when the engine publishes a new volume list (mount
/// changes, capacity every 10 s).
pub fn volumes_panel(
    state: AppState,
    tint: Memo<Color>,
    show_system: Signal<bool>,
    drives: DriveActions,
) -> impl View {
    let live = state.live;
    let list = keyed(
        move |runtime| {
            let shown = show_system.get(runtime);
            let volumes = live.with(runtime, |snapshot| {
                Arc::as_ptr(&snapshot.tick.disk.volumes).cast::<()>() as usize
            });
            (volumes, shown)
        },
        move |cx, (_, shown)| {
            live.with_untracked(cx, |snapshot| {
                volume_list(&snapshot.tick.disk.volumes, shown, tint, drives)
            })
        },
    );
    vstack((toggle(show_system).caption("Show system volumes"), list))
        .gap(spacing::LOOSE)
        .w_full()
}
