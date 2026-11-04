use std::fmt::Write;
use std::sync::Arc;

use crownui::kit::{StatusOptions, Tone, icons, status};
use crownui::prelude::{
    AnyView, Role, Runtime, Semantic, Styled, View, bind, hstack, keyed, spacer, text, vstack,
};
use monitor_engine::DriveAction;
use monitor_engine::io::{DeviceInfo, DiskDevice, MediaKind, StackKind, Transport};

use super::drive_actions::DriveActions;
use super::health::health_card;
use crate::app::{AppState, EngineLink};
use crate::components::empty_state::empty_state;
use crate::components::fact_row::fact_row;
use crate::settings::MonitorSettings;
use crate::util::format::{bytes, percent, rate, temperature};
use crate::util::spacing;

const NAME_SIZE: f32 = 14.0;

pub const fn transport_name(transport: Transport) -> &'static str {
    match transport {
        Transport::Nvme => "NVMe",
        Transport::Sata => "SATA",
        Transport::Scsi => "SCSI",
        Transport::Usb => "USB",
        Transport::Virtio => "Virtual",
        Transport::Mmc => "SD/MMC",
        Transport::Other => "Other",
    }
}

pub const fn media_name(media: MediaKind) -> &'static str {
    match media {
        MediaKind::Ssd => "SSD",
        MediaKind::Hdd => "HDD",
        MediaKind::Unknown => "",
    }
}

const fn stack_name(kind: StackKind) -> &'static str {
    match kind {
        StackKind::Luks => "LUKS",
        StackKind::Lvm => "LVM",
        StackKind::Md => "RAID",
        StackKind::Bcache => "bcache",
        StackKind::DeviceMapper => "device-mapper",
        StackKind::Other => "virtual",
    }
}

/// Partitions, then LUKS/LVM/MD devices indented under what they sit on.
pub fn stacking_tree(info: &DeviceInfo) -> String {
    let mut tree = String::new();
    for partition in &info.partitions {
        let _ = writeln!(tree, "{partition}");
        for stacked in info
            .stack
            .iter()
            .filter(|stacked| stacked.parent == *partition)
        {
            write_stacked(&mut tree, info, stacked.name.as_ref(), 1);
        }
    }
    for stacked in info
        .stack
        .iter()
        .filter(|stacked| *stacked.parent == *info.key.as_str())
    {
        write_stacked(&mut tree, info, stacked.name.as_ref(), 0);
    }
    tree.trim_end().to_owned()
}

fn write_stacked(tree: &mut String, info: &DeviceInfo, name: &str, depth: usize) {
    let Some(device) = info.stack.iter().find(|stacked| &*stacked.name == name) else {
        return;
    };
    let mapped = device
        .mapped_name
        .as_deref()
        .map_or_else(String::new, |mapped| format!(" ({mapped})"));
    let _ = writeln!(
        tree,
        "{}└ {}{mapped} · {}",
        "  ".repeat(depth),
        device.name,
        stack_name(device.kind)
    );
    for child in info.stack.iter().filter(|child| *child.parent == *name) {
        write_stacked(tree, info, &child.name, depth + 1);
    }
}

fn device_figure(
    state: AppState,
    index: usize,
    write: fn(&DiskDevice, MonitorSettings) -> String,
) -> impl Fn(&mut Runtime) -> String + 'static {
    let (live, settings) = (state.live, state.settings);
    move |runtime| {
        let settings = settings.get(runtime);
        live.with(runtime, |snapshot| {
            snapshot
                .tick
                .disk
                .devices
                .get(index)
                .map_or_else(String::new, |device| write(device, settings))
        })
    }
}

fn throughput(device: &DiskDevice, settings: MonitorSettings) -> String {
    let unit = settings.rates;
    format!(
        "{} read · {} written",
        rate(device.rates.read.get(), unit),
        rate(device.rates.write.get(), unit)
    )
}

fn operations(device: &DiskDevice, _: MonitorSettings) -> String {
    let rates = &device.rates;
    format!(
        "{:.0} reads/s · {:.0} writes/s · {:.0} discards/s",
        rates.read_iops, rates.write_iops, rates.discards_per_second
    )
}

fn latency(device: &DiskDevice, _: MonitorSettings) -> String {
    let shown =
        |latency: Option<f32>| latency.map_or_else(|| "–".to_owned(), |ms| format!("{ms:.2} ms"));
    format!(
        "{} read · {} write",
        shown(device.rates.read_latency_ms),
        shown(device.rates.write_latency_ms)
    )
}

fn busy(device: &DiskDevice, _: MonitorSettings) -> String {
    format!(
        "{} busy · {} in flight",
        percent(device.rates.utilization.get()),
        device.rates.in_flight
    )
}

fn heat(device: &DiskDevice, settings: MonitorSettings) -> String {
    let scale = settings.temperature;
    device.temperature.map_or_else(
        || "–".to_owned(),
        |celsius| temperature(celsius.get(), scale),
    )
}

fn since_boot(device: &DiskDevice, _: MonitorSettings) -> String {
    format!(
        "{} read · {} written",
        bytes(device.read_total.get()),
        bytes(device.written_total.get())
    )
}

fn device_card(
    state: AppState,
    index: usize,
    info: &DeviceInfo,
    link: Option<EngineLink>,
    drives: DriveActions,
) -> AnyView {
    let name = info.key.as_str().to_owned();
    let model = info.model.as_deref().unwrap_or("Unknown drive").to_owned();
    let mut kinds = vec![transport_name(info.transport)];
    kinds.extend(Some(media_name(info.media)).filter(|media| !media.is_empty()));
    let kind_badges: Vec<AnyView> = kinds
        .into_iter()
        .map(|kind| status(kind).tone(Tone::Neutral).boxed())
        .collect();
    let removable = info.removable.then(|| {
        hstack((
            status("Removable").tone(Tone::Neutral),
            drives.button(
                Arc::from(info.key.as_str()),
                Arc::from(format!("{model} ({name})")),
                DriveAction::PowerOff,
            ),
        ))
        .gap(spacing::SNUG)
        .items_center()
    });
    let tree = stacking_tree(info);
    let sectors = format!(
        "{} B logical · {} B physical",
        info.logical_sector_size, info.physical_sector_size
    );
    vstack((
        hstack((
            text(format!("{model} · {name}"))
                .text_size(NAME_SIZE)
                .font_semibold()
                .truncate(),
            spacer(),
            hstack(kind_badges).gap(spacing::SNUG),
            removable,
        ))
        .gap(spacing::COMFORTABLE)
        .items_center(),
        fact_row("Throughput", bind(device_figure(state, index, throughput))),
        fact_row("Operations", bind(device_figure(state, index, operations))),
        fact_row("Latency", bind(device_figure(state, index, latency))),
        fact_row("Utilization", bind(device_figure(state, index, busy))),
        fact_row("Temperature", bind(device_figure(state, index, heat))),
        fact_row("Since boot", bind(device_figure(state, index, since_boot))),
        fact_row("Capacity", bytes(info.size.get())),
        fact_row("Serial", info.serial.as_deref().unwrap_or("–").to_owned()),
        fact_row(
            "Firmware",
            info.firmware.as_deref().unwrap_or("–").to_owned(),
        ),
        fact_row("Sector size", sectors),
        fact_row(
            "Scheduler",
            info.scheduler.as_deref().unwrap_or("–").to_owned(),
        ),
        fact_row(
            "Layout",
            if tree.is_empty() {
                "–".to_owned()
            } else {
                tree
            },
        ),
        health_card(state, index, link),
    ))
    .gap(spacing::SNUG)
    .w_full()
    .role(Role::Group)
    .label(name)
    .boxed()
}

/// One card per physical disk: live service figures, the drive itself, its stacking and health;
/// removable drives offer Power Off. Rebuilt only when the set of disks changes.
pub fn devices_panel(state: AppState, drives: DriveActions) -> impl View {
    let live = state.live;
    keyed(
        move |runtime| {
            live.with(runtime, |snapshot| {
                snapshot
                    .tick
                    .disk
                    .devices
                    .iter()
                    .map(|device| Arc::as_ptr(&device.info) as usize)
                    .fold(0_usize, |hash, pointer| hash.rotate_left(7) ^ pointer)
            })
        },
        move |cx, _| {
            let link = EngineLink::find(cx);
            let cards: Vec<AnyView> = live.with_untracked(cx, |snapshot| {
                snapshot
                    .tick
                    .disk
                    .devices
                    .iter()
                    .enumerate()
                    .map(|(index, device)| {
                        device_card(state, index, &device.info, link.clone(), drives)
                    })
                    .collect()
            });
            if cards.is_empty() {
                return empty_state(
                    icons::HARD_DRIVE,
                    "No disks found",
                    "No physical disks report I/O.",
                )
                .boxed();
            }
            vstack(cards).gap(spacing::GENEROUS).w_full().boxed()
        },
    )
}

#[cfg(test)]
mod tests {
    use monitor_core::{Bytes, DeviceKey};
    use monitor_engine::io::StackedDevice;

    use super::*;

    fn info() -> DeviceInfo {
        DeviceInfo {
            key: DeviceKey::new("nvme0n1"),
            model: None,
            serial: None,
            firmware: None,
            size: Bytes::ZERO,
            media: MediaKind::Ssd,
            removable: false,
            transport: Transport::Nvme,
            logical_sector_size: 512,
            physical_sector_size: 512,
            scheduler: None,
            partitions: vec!["nvme0n1p1".into(), "nvme0n1p2".into()],
            stack: vec![
                StackedDevice {
                    name: "dm-0".into(),
                    mapped_name: Some("cryptroot".into()),
                    kind: StackKind::Luks,
                    parent: "nvme0n1p2".into(),
                    depth: 1,
                },
                StackedDevice {
                    name: "dm-1".into(),
                    mapped_name: Some("vg-home".into()),
                    kind: StackKind::Lvm,
                    parent: "dm-0".into(),
                    depth: 2,
                },
            ],
        }
    }

    #[test]
    fn stacks_hang_under_their_partition() {
        assert_eq!(
            stacking_tree(&info()),
            "nvme0n1p1\nnvme0n1p2\n  └ dm-0 (cryptroot) · LUKS\n    └ dm-1 (vg-home) · LVM"
        );
    }
}
