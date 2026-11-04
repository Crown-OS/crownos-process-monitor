//! GPU enumeration from `/sys/class/drm/card*`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use monitor_core::util::parse;
use monitor_core::util::sysfs;
use monitor_core::{Bytes, DeviceKey};

use super::backend::Backend;
use super::gpu_metrics::{self, MetricsFamily};
use super::paths::SystemPaths;
use super::snapshot::{
    BackendKind, DriverSupport, GpuIdentity, GpuKind, GpuPower, GpuVendor, PciIdentity,
};
use super::wake::RuntimePmGuard;
use crate::util::{amdgpu_ids, attribute, directory, pci_ids};

/// Firmware framebuffers and virtual devices are not GPUs worth monitoring.
const IGNORED_DRIVERS: [&str; 4] = ["simpledrm", "simple-framebuffer", "ofdrm", "vkms"];
/// An AMD part with less dedicated memory than this is an APU carve-out.
const APU_CARVE_OUT_LIMIT: Bytes = Bytes::new(2 * 1024 * 1024 * 1024);

#[derive(Debug)]
pub(crate) struct DiscoveredGpu {
    pub(crate) card_directory: PathBuf,
    pub(crate) device_directory: PathBuf,
    pub(crate) identity: GpuIdentity,
}

pub(crate) fn is_card(name: &str) -> bool {
    directory::is_numbered(name, "card")
}

pub(crate) fn signature(paths: &SystemPaths) -> u64 {
    directory::signature(&paths.drm(), is_card)
}

pub(crate) fn discover(paths: &SystemPaths, buffer: &mut Vec<u8>) -> Vec<DiscoveredGpu> {
    let drm = paths.drm();
    let mut gpus: Vec<DiscoveredGpu> = directory::entries(&drm, is_card)
        .into_iter()
        .filter_map(|card| inspect(paths, &drm, &card, buffer))
        .collect();
    gpus.dedup_by(|duplicate, kept| duplicate.identity.key == kept.identity.key);
    gpus
}

fn inspect(
    paths: &SystemPaths,
    drm: &Path,
    card: &str,
    buffer: &mut Vec<u8>,
) -> Option<DiscoveredGpu> {
    let card_directory = drm.join(card);
    let device_directory = std::fs::canonicalize(card_directory.join("device")).ok()?;
    let driver = sysfs::link_name(device_directory.join("driver"))?;
    if IGNORED_DRIVERS.contains(&driver.as_str()) {
        return None;
    }
    let key = DeviceKey::new(device_directory.file_name()?.to_str()?);
    let read_id = |name: &str| {
        attribute::read_hex(device_directory.join(name)).and_then(|id| u16::try_from(id).ok())
    };
    let vendor_id = read_id("vendor");
    let pci = PciIdentity {
        vendor: vendor_id.unwrap_or(0),
        device: read_id("device").unwrap_or(0),
        subsystem_vendor: read_id("subsystem_vendor").unwrap_or(0),
        subsystem_device: read_id("subsystem_device").unwrap_or(0),
        revision: attribute::read_hex(device_directory.join("revision"))
            .and_then(|revision| u8::try_from(revision).ok())
            .unwrap_or(0),
    };
    let is_pci = vendor_id.is_some();
    let vendor = GpuVendor::from_pci(pci.vendor);
    let backend = Backend::kind_for_driver(&driver);
    let active = RuntimePmGuard::open(&device_directory).state(buffer) == GpuPower::Active;
    let kind = classify(vendor, is_pci, &key, &device_directory, active, buffer);
    let name = if is_pci {
        pci_name(paths, vendor, &pci)
    } else {
        Arc::from(format!("{driver} ({key})"))
    };
    let vbios = (active && backend == BackendKind::Amd)
        .then(|| attribute::read_text(device_directory.join("vbios_version")))
        .flatten();
    let driver_version = attribute::read_text(paths.module_version(&driver))
        .or_else(|| attribute::read_text(&paths.kernel_release));
    let support = match backend {
        BackendKind::Nouveau => DriverSupport::Limited,
        BackendKind::Generic if driver == "radeon" => DriverSupport::Limited,
        _ => DriverSupport::Full,
    };
    let identity = GpuIdentity {
        key,
        card: Arc::from(card),
        name,
        vendor,
        pci,
        driver: Arc::from(driver),
        driver_version,
        vbios,
        kind,
        boot_vga: attribute::read_number(device_directory.join("boot_vga")) == Some(1),
        connected_displays: connected_displays(drm, card),
        backend,
        support,
    };
    Some(DiscoveredGpu {
        card_directory,
        device_directory,
        identity,
    })
}

fn classify(
    vendor: GpuVendor,
    is_pci: bool,
    key: &DeviceKey,
    device: &Path,
    active: bool,
    buffer: &mut Vec<u8>,
) -> GpuKind {
    if !is_pci {
        return GpuKind::Integrated;
    }
    match vendor {
        GpuVendor::Intel if pci_bus(key) == Some(0) => GpuKind::Integrated,
        GpuVendor::Amd if active && amd_is_apu(device, buffer) => GpuKind::Integrated,
        _ => GpuKind::Discrete,
    }
}

/// APUs publish the v2/v3 `gpu_metrics` layouts; without the table, a small
/// VRAM carve-out gives them away.
fn amd_is_apu(device: &Path, buffer: &mut Vec<u8>) -> bool {
    let header = sysfs::read_into(device.join("gpu_metrics"), buffer)
        .ok()
        .and_then(gpu_metrics::header);
    match header {
        Some(header) if header.format_revision >= 2 => true,
        Some(header) if gpu_metrics::family(header) == Some(MetricsFamily::Discrete) => false,
        _ => attribute::read_number(device.join("mem_info_vram_total"))
            .is_some_and(|total| Bytes::new(total) < APU_CARVE_OUT_LIMIT),
    }
}

fn pci_bus(key: &DeviceKey) -> Option<u32> {
    let bus = key.as_str().split(':').nth(1)?;
    parse::parse_hex_u32(bus.as_bytes())
}

fn pci_name(paths: &SystemPaths, vendor: GpuVendor, pci: &PciIdentity) -> Arc<str> {
    if vendor == GpuVendor::Amd
        && let Some(name) = amdgpu_ids::lookup_file(&paths.amdgpu_ids, pci.device, pci.revision)
    {
        return name;
    }
    let names = pci_ids::lookup_file(&paths.pci_ids, pci.vendor, pci.device);
    let prefix = vendor.short_name().map(str::to_owned).or(names.vendor);
    let device = names
        .device
        .as_deref()
        .map(pci_ids::marketing_name)
        .map_or_else(
            || format!("GPU {:04x}:{:04x}", pci.vendor, pci.device),
            str::to_owned,
        );
    match prefix {
        Some(prefix) if !device.starts_with(&prefix) => Arc::from(format!("{prefix} {device}")),
        _ => Arc::from(device),
    }
}

/// Connector `status` is the cached hotplug state; reading it never probes.
pub(crate) fn connected_displays(drm: &Path, card: &str) -> Arc<[Arc<str>]> {
    let prefix = format!("{card}-");
    directory::entries(drm, |name| name.starts_with(&prefix))
        .into_iter()
        .filter(|connector| {
            sysfs::read_small(drm.join(connector).join("status"), |status| {
                Some(status == b"connected")
            })
            .unwrap_or(false)
        })
        .map(|connector| Arc::from(&connector[prefix.len()..]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pci_bus() {
        assert_eq!(pci_bus(&DeviceKey::new("0000:00:02.0")), Some(0));
        assert_eq!(pci_bus(&DeviceKey::new("0000:06:00.0")), Some(6));
        assert_eq!(pci_bus(&DeviceKey::new("fde60000.gpu")), None);
    }

    #[test]
    fn recognises_cards() {
        assert!(is_card("card1"));
        assert!(!is_card("card1-eDP-1"));
        assert!(!is_card("renderD128"));
    }
}
