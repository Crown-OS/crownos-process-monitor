//! The Disk page over a seeded tick: one NVMe drive, a root volume and a USB stick.

use std::sync::Arc;

use crowntest::{Role, role, text};
use monitor_core::{Bytes, BytesPerSecond, Coverage, DeviceKey, Percent};
use monitor_engine::Snapshot;
use monitor_engine::io::{
    Capacity, DeviceInfo, DeviceNumber, DiskDevice, DiskRates, DiskSnapshot, DriveHealth,
    IoPressure, MediaKind, MountPoint, NvmeHealth, Transport, Volume, VolumeLocation, VolumeRole,
};

use super::page;
use crate::process_table::test_support::{live_with, window};

/// Whether at least one node matches; text often repeats (a panel and a row share a title).
fn shows(app: &mut crowntest::TestApp, query: crowntest::Query) -> bool {
    app.count(query) > 0
}

const GIB: u64 = 1 << 30;

fn volume(mount: &str, block: &str, removable: bool) -> Volume {
    Volume {
        device: DeviceNumber::default(),
        source: format!("/dev/{block}").into(),
        fs_type: "ext4".into(),
        label: None,
        block_device: Some(block.into()),
        disk: Some("nvme0n1".into()),
        mount_points: vec![MountPoint {
            path: mount.into(),
            root: "/".into(),
            read_only: false,
        }],
        read_only: false,
        removable,
        role: VolumeRole::User,
        location: VolumeLocation::Local,
        capacity: Some(Capacity {
            size: Bytes::new(512 * GIB),
            used: Bytes::new(200 * GIB),
            free: Bytes::new(312 * GIB),
            available: Bytes::new(300 * GIB),
            coverage: Coverage::Full,
        }),
    }
}

fn seeded_disk() -> DiskSnapshot {
    let info = DeviceInfo {
        key: DeviceKey::new("nvme0n1"),
        model: Some("Samsung 990 PRO".into()),
        serial: Some("S7X1".into()),
        firmware: Some("4B2QJXD7".into()),
        size: Bytes::new(1024 * GIB),
        media: MediaKind::Ssd,
        removable: false,
        transport: Transport::Nvme,
        logical_sector_size: 512,
        physical_sector_size: 4096,
        scheduler: Some("none".into()),
        partitions: vec!["nvme0n1p1".into()],
        stack: Vec::new(),
    };
    let health = NvmeHealth {
        percent_used: Some(Percent::new(3.0)),
        available_spare: Some(Percent::new(100.0)),
        spare_threshold: Some(Percent::new(10.0)),
        media_errors: Some(0),
        power_on_hours: Some(1200),
        data_written: Some(Bytes::new(40 * 1024 * GIB)),
        ..NvmeHealth::default()
    };
    DiskSnapshot {
        devices: vec![DiskDevice {
            info: Arc::new(info),
            rates: DiskRates {
                read: BytesPerSecond::new(2e6),
                write: BytesPerSecond::new(5e5),
                read_iops: 120.0,
                read_latency_ms: Some(0.3),
                utilization: Percent::new(12.0),
                in_flight: 2,
                ..DiskRates::default()
            },
            read_total: Bytes::new(80 * GIB),
            written_total: Bytes::new(20 * GIB),
            temperature: None,
            health: DriveHealth::Nvme(Arc::new(health)),
        }],
        volumes: Arc::from(vec![
            volume("/", "nvme0n1p1", false),
            volume("/run/media/usb", "sda1", true),
        ]),
        pressure: Some(IoPressure {
            some_avg10: Percent::new(1.5),
            ..IoPressure::default()
        }),
        ..DiskSnapshot::default()
    }
}

#[test]
fn disk_renders_its_sections_from_the_tick() {
    let tick = Snapshot {
        disk: seeded_disk(),
        ..Snapshot::default()
    };
    let mut app = window(live_with(tick), page);
    app.settle();
    for title in [
        "Throughput",
        "Read and written",
        "Volumes",
        "Disks",
        "I/O pressure",
        "Apps",
    ] {
        assert!(
            shows(&mut app, text(title)),
            "no {title} panel:\n{}",
            app.semantics_snapshot()
        );
    }
    assert!(app.exists(role(Role::ListItem).name("/")));
    assert!(app.exists(role(Role::ListItem).name("/run/media/usb")));
    assert!(app.exists(role(Role::Group).name("nvme0n1")));
    assert!(shows(&mut app, text("Samsung 990 PRO")));
    assert!(app.exists(role(Role::Group).name("Health")));
    assert!(shows(&mut app, text("Healthy")));
    assert!(app.exists(role(Role::Group).name("Percentage used")));
    assert!(shows(&mut app, text("1.5% over 10 s")));
    assert!(
        app.exists(role(Role::Button).name("Unmount")),
        "removable volumes offer Unmount"
    );
    app.click(role(Role::Button).name("Unmount"));
    app.settle();
    assert!(
        shows(&mut app, text("Unmount /run/media/usb?")),
        "Unmount asks first:\n{}",
        app.semantics_snapshot()
    );
    assert!(
        app.exists(role(Role::ListItem).name("Firefox")),
        "the app table"
    );
}
