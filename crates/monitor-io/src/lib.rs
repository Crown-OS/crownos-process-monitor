//! Disks, volumes, drive health, network interfaces, sockets and Wi-Fi for
//! the CrownOS process monitor. Every collector samples without blocking:
//! D-Bus work (UDisks2, NetworkManager, resolved) runs on background threads.

pub mod disk;
mod error;
pub mod net;
mod rate;
mod util;

pub use disk::{
    AtaAttribute, AtaHealth, AttributeUnit, Capacity, DeviceInfo, DeviceNumber, DiskCollector,
    DiskDevice, DiskRates, DiskSnapshot, DriveHealth, HealthVerdict, IoPressure, MediaKind,
    MountPoint, NvmeHealth, RequestReceiver, StackKind, StackedDevice, Transport, UDisks, Volume,
    VolumeLocation, VolumeRole,
};
pub use error::IoError;
pub use net::{
    BindScope, Connection, DefaultRoute, DnsServers, DnsSource, Duplex, InterfaceAddress,
    InterfaceCounters, InterfaceInfo, InterfaceKind, InterfaceRates, ListeningPort,
    NetworkCollector, NetworkInterface, NetworkSnapshot, OperState, ProcessTraffic, Protocol,
    TcpState, WifiBand, WifiInfo, WifiSecurity, WifiSource, wants_socket_owners,
};
pub use rate::RateStatus;
