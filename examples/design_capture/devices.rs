//! Storage and networking of the sample laptop: an NVMe drive, a USB stick, Wi-Fi and a VPN.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use monitor_core::{Bytes, BytesPerSecond, Coverage, DeviceKey, Percent, Pid, Uid};
use monitor_engine::io::{
    BindScope, Capacity, Connection, DefaultRoute, DeviceInfo, DeviceNumber, DiskDevice, DiskRates,
    DiskSnapshot, DnsServers, DnsSource, DriveHealth, Duplex, InterfaceAddress, InterfaceCounters,
    InterfaceInfo, InterfaceKind, InterfaceRates, IoPressure, ListeningPort, MediaKind, MountPoint,
    NetworkInterface, NetworkSnapshot, NvmeHealth, OperState, ProcessTraffic, Protocol, RateStatus,
    TcpState, Transport, Volume, VolumeLocation, VolumeRole, WifiBand, WifiInfo, WifiSecurity,
};

use crate::apps::USER_UID;

const GIB: u64 = 1 << 30;
const MIB: f64 = (1 << 20) as f64;

/// Read and write rates for one tick, in bytes per second.
#[derive(Clone, Copy)]
pub struct DiskLoad {
    pub read: f64,
    pub write: f64,
}

/// Download and upload for one tick, in bytes per second, plus the tunnel's share.
#[derive(Clone, Copy)]
pub struct NetworkLoad {
    pub download: f64,
    pub upload: f64,
    pub vpn: f64,
}

fn volume(
    mount: &str,
    block: &str,
    disk: &str,
    (size_gib, used_gib): (u64, u64),
    removable: bool,
) -> Volume {
    Volume {
        device: DeviceNumber::default(),
        source: format!("/dev/{block}").into(),
        fs_type: if removable { "exfat" } else { "btrfs" }.into(),
        label: removable.then(|| "PHOTOS".into()),
        block_device: Some(block.into()),
        disk: Some(disk.into()),
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
            size: Bytes::new(size_gib * GIB),
            used: Bytes::new(used_gib * GIB),
            free: Bytes::new((size_gib - used_gib) * GIB),
            available: Bytes::new((size_gib - used_gib) * GIB),
            coverage: Coverage::Full,
        }),
    }
}

fn nvme_drive(load: DiskLoad) -> DiskDevice {
    let info = DeviceInfo {
        key: DeviceKey::new("nvme0n1"),
        model: Some("Samsung 990 PRO 1TB".into()),
        serial: Some("S7KHNJ0W512345".into()),
        firmware: Some("4B2QJXD7".into()),
        size: Bytes::new(1000 * GIB),
        media: MediaKind::Ssd,
        removable: false,
        transport: Transport::Nvme,
        logical_sector_size: 512,
        physical_sector_size: 4096,
        scheduler: Some("none".into()),
        partitions: vec!["nvme0n1p1".into(), "nvme0n1p2".into()],
        stack: Vec::new(),
    };
    let health = NvmeHealth {
        percent_used: Some(Percent::new(3.0)),
        available_spare: Some(Percent::new(100.0)),
        spare_threshold: Some(Percent::new(10.0)),
        media_errors: Some(0),
        power_on_hours: Some(1840),
        data_written: Some(Bytes::new(38 * 1024 * GIB)),
        ..NvmeHealth::default()
    };
    DiskDevice {
        info: Arc::new(info),
        rates: rates(load, 1.0),
        read_total: Bytes::new(82 * GIB),
        written_total: Bytes::new(24 * GIB),
        temperature: Some(monitor_core::Celsius::new(41.0)),
        health: DriveHealth::Nvme(Arc::new(health)),
    }
}

fn usb_stick() -> DiskDevice {
    DiskDevice {
        info: Arc::new(DeviceInfo {
            key: DeviceKey::new("sda"),
            model: Some("SanDisk Ultra Fit".into()),
            serial: None,
            firmware: Some("1.00".into()),
            size: Bytes::new(64 * GIB),
            media: MediaKind::Unknown,
            removable: true,
            transport: Transport::Usb,
            logical_sector_size: 512,
            physical_sector_size: 512,
            scheduler: Some("mq-deadline".into()),
            partitions: vec!["sda1".into()],
            stack: Vec::new(),
        }),
        rates: DiskRates::default(),
        read_total: Bytes::new(2 * GIB),
        written_total: Bytes::new(GIB / 4),
        temperature: None,
        health: DriveHealth::Unavailable,
    }
}

fn rates(load: DiskLoad, share: f64) -> DiskRates {
    DiskRates {
        read: BytesPerSecond::new(load.read * share),
        write: BytesPerSecond::new(load.write * share),
        read_iops: (load.read / 65_536.0) as f32,
        write_iops: (load.write / 32_768.0) as f32,
        read_latency_ms: Some(0.08),
        write_latency_ms: Some(0.21),
        utilization: Percent::new(((load.read + load.write) / (24.0 * MIB)).min(100.0) as f32),
        in_flight: 1,
        discards_per_second: 0.0,
    }
}

pub fn disk(load: DiskLoad) -> DiskSnapshot {
    DiskSnapshot {
        status: RateStatus::Measured,
        devices: vec![nvme_drive(load), usb_stick()],
        totals: rates(load, 1.0),
        read_total: Bytes::new(84 * GIB),
        written_total: Bytes::new(24 * GIB),
        volumes: Arc::from(vec![
            volume("/", "nvme0n1p2", "nvme0n1", (930, 412), false),
            volume("/run/media/alex/PHOTOS", "sda1", "sda", (59, 21), true),
        ]),
        pressure: Some(IoPressure {
            some_avg10: Percent::new(0.6),
            some_avg60: Percent::new(0.4),
            ..IoPressure::default()
        }),
    }
}

fn address(octets: [u8; 4]) -> IpAddr {
    IpAddr::V4(Ipv4Addr::from(octets))
}

fn interface(
    (index, name, kind): (u32, &str, InterfaceKind),
    addresses: &[[u8; 4]],
    state: OperState,
    (receive, transmit): (f64, f64),
) -> NetworkInterface {
    NetworkInterface {
        info: Arc::new(InterfaceInfo {
            index,
            name: name.into(),
            kind,
            link_kind: None,
            mac: (kind != InterfaceKind::Loopback)
                .then(|| format!("02:5e:7c:4a:1b:{index:02x}").into()),
            mtu: Some(if kind == InterfaceKind::Vpn {
                1280
            } else {
                1500
            }),
            speed_mbps: (kind == InterfaceKind::Ethernet).then_some(1000),
            duplex: if kind == InterfaceKind::Ethernet {
                Duplex::Full
            } else {
                Duplex::Unknown
            },
            addresses: addresses
                .iter()
                .map(|octets| InterfaceAddress {
                    address: address(*octets),
                    prefix_length: if octets[0] == 100 { 32 } else { 24 },
                })
                .collect(),
        }),
        state,
        counters: InterfaceCounters {
            receive_bytes: Bytes::new((receive * 5400.0) as u64),
            transmit_bytes: Bytes::new((transmit * 5400.0) as u64),
            ..InterfaceCounters::default()
        },
        rates: InterfaceRates {
            receive: BytesPerSecond::new(receive),
            transmit: BytesPerSecond::new(transmit),
            receive_packets: (receive / 1200.0) as f32,
            transmit_packets: (transmit / 600.0) as f32,
            ..InterfaceRates::default()
        },
    }
}

fn listening(port: u16, octets: [u8; 4], owner: Option<u32>, uid: u32) -> ListeningPort {
    let address = address(octets);
    ListeningPort {
        protocol: Protocol::Tcp,
        address: SocketAddr::new(address, port),
        scope: BindScope::of(address),
        owner: owner.map(Pid::new),
        uid: Uid::new(uid),
        inode: u64::from(port),
    }
}

fn connection(local_port: u16, remote: ([u8; 4], u16), owner: u32, state: TcpState) -> Connection {
    Connection {
        protocol: Protocol::Tcp,
        local: SocketAddr::new(address([192, 168, 1, 42]), local_port),
        remote: SocketAddr::new(address(remote.0), remote.1),
        state,
        receive_queue: 0,
        send_queue: 0,
        owner: Some(Pid::new(owner)),
        uid: Uid::new(USER_UID),
        inode: u64::from(local_port),
    }
}

pub fn network(load: NetworkLoad) -> NetworkSnapshot {
    let wifi = interface(
        (2, "wlo1", InterfaceKind::Wifi),
        &[[192, 168, 1, 42]],
        OperState::Up,
        (load.download, load.upload),
    );
    let vpn = interface(
        (4, "tailscale0", InterfaceKind::Vpn),
        &[[100, 101, 7, 12]],
        OperState::Up,
        (load.vpn, load.vpn / 4.0),
    );
    NetworkSnapshot {
        status: RateStatus::Measured,
        interfaces: vec![
            interface(
                (1, "lo", InterfaceKind::Loopback),
                &[[127, 0, 0, 1]],
                OperState::Up,
                (0.0, 0.0),
            ),
            wifi,
            interface(
                (3, "enp5s0", InterfaceKind::Ethernet),
                &[],
                OperState::Down,
                (0.0, 0.0),
            ),
            vpn,
        ],
        totals: InterfaceRates {
            receive: BytesPerSecond::new(load.download),
            transmit: BytesPerSecond::new(load.upload),
            ..InterfaceRates::default()
        },
        vpn_totals: InterfaceRates {
            receive: BytesPerSecond::new(load.vpn),
            transmit: BytesPerSecond::new(load.vpn / 4.0),
            ..InterfaceRates::default()
        },
        receive_total: Bytes::new(6 * GIB),
        transmit_total: Bytes::new(GIB / 2),
        default_routes: Arc::from(vec![DefaultRoute {
            gateway: address([192, 168, 1, 1]),
            interface_index: 2,
            interface: Some("wlo1".into()),
        }]),
        dns: DnsServers {
            servers: Arc::from(vec![address([192, 168, 1, 1]), address([1, 1, 1, 1])]),
            source: DnsSource::Resolved,
        },
        wifi: Arc::from(vec![WifiInfo {
            interface: "wlo1".into(),
            ssid: Some("Crown Home".into()),
            strength: Some(Percent::new(74.0)),
            signal_dbm: Some(-53),
            band: WifiBand::FiveGhz,
            channel: Some(44),
            link_rate_mbps: Some(1201),
            max_rate_mbps: Some(2402),
            security: WifiSecurity::Wpa3Personal,
            ..WifiInfo::default()
        }]),
        listening: vec![
            listening(22, [0, 0, 0, 0], None, 0),
            listening(631, [127, 0, 0, 1], None, 0),
            listening(5432, [127, 0, 0, 1], Some(4020), USER_UID),
            listening(57621, [0, 0, 0, 0], Some(2550), USER_UID),
        ],
        connections: vec![
            connection(51234, ([203, 0, 113, 24], 443), 2398, TcpState::Established),
            connection(51240, ([203, 0, 113, 77], 443), 2311, TcpState::Established),
            connection(
                51302,
                ([198, 51, 100, 14], 443),
                2550,
                TcpState::Established,
            ),
            connection(
                51388,
                ([198, 51, 100, 201], 443),
                2804,
                TcpState::Established,
            ),
            connection(51410, ([203, 0, 113, 9], 80), 2402, TcpState::TimeWait),
        ],
        process_traffic: [(2398, 0.62), (2311, 0.18), (2550, 0.12), (2804, 0.02)]
            .into_iter()
            .map(|(pid, share)| ProcessTraffic {
                pid: Pid::new(pid),
                receive: BytesPerSecond::new(load.download * share),
                transmit: BytesPerSecond::new(load.upload * share),
                received: Bytes::new((load.download * share * 3600.0) as u64),
                transmitted: Bytes::new((load.upload * share * 3600.0) as u64),
            })
            .collect(),
        traffic_passes: 1,
        process_traffic_coverage: Coverage::TcpOnly,
    }
}
