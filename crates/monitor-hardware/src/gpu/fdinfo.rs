//! Parser for the DRM client usage keys of `/proc/<pid>/fdinfo/<fd>`, as
//! specified by the kernel's `Documentation/gpu/drm-usage-stats.rst`.

use monitor_core::util::parse;

use super::engine::EngineClass;

const KIB: u64 = 1024;
const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * 1024 * 1024;

/// Busy time of every engine of one class, as read from a single client.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EngineCounter {
    /// Nanoseconds (`drm-engine-*`) or busy cycles (`drm-cycles-*`).
    pub(crate) busy: u64,
    /// Elapsed GPU cycles (`drm-total-cycles-*`); zero for time-based engines.
    pub(crate) total_cycles: u64,
    names: u32,
    extra_capacity: u32,
}

impl EngineCounter {
    pub(crate) const fn is_present(&self) -> bool {
        self.names > 0
    }

    /// Engine instances behind the counter (`drm-engine-capacity-*`).
    pub(crate) fn capacity(&self) -> u32 {
        (self.names + self.extra_capacity).max(1)
    }

    pub(crate) const fn counts_cycles(&self) -> bool {
        self.total_cycles > 0
    }
}

pub(crate) type EngineCounters = [EngineCounter; EngineClass::COUNT];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RegionSum {
    rank: u8,
    bytes: u64,
}

impl RegionSum {
    fn add(&mut self, rank: u8, bytes: u64) {
        if rank > self.rank {
            *self = Self { rank, bytes };
        } else if rank == self.rank {
            self.bytes = self.bytes.saturating_add(bytes);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegionKind {
    Local,
    Shared,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DrmClient<'a> {
    pub(crate) driver: Option<&'a [u8]>,
    pub(crate) pdev: Option<&'a [u8]>,
    pub(crate) client_id: Option<u64>,
    pub(crate) engines: EngineCounters,
    local_memory: RegionSum,
    shared_memory: RegionSum,
}

impl DrmClient<'_> {
    pub(crate) const fn local_memory(&self) -> u64 {
        self.local_memory.bytes
    }

    pub(crate) const fn shared_memory(&self) -> u64 {
        self.shared_memory.bytes
    }

    pub(crate) fn has_usage(&self) -> bool {
        self.engines.iter().any(EngineCounter::is_present)
            || self.local_memory.bytes > 0
            || self.shared_memory.bytes > 0
    }
}

/// Resident memory beats the legacy `drm-memory-` key, which beats totals.
const RESIDENT_RANK: u8 = 3;
const LEGACY_RANK: u8 = 2;
const TOTAL_RANK: u8 = 1;

pub(crate) fn parse(fdinfo: &[u8]) -> DrmClient<'_> {
    let mut client = DrmClient::default();
    for (key, value) in parse::lines(fdinfo).filter_map(parse::key_value) {
        let Some(key) = key.strip_prefix(b"drm-") else {
            continue;
        };
        apply(&mut client, key, value);
    }
    client
}

fn apply<'a>(client: &mut DrmClient<'a>, key: &[u8], value: &'a [u8]) {
    let number = || parse::fields(value).next().and_then(parse::parse_u64);
    if key == b"driver" {
        client.driver = Some(value);
    } else if key == b"pdev" {
        client.pdev = Some(value);
    } else if key == b"client-id" {
        client.client_id = number();
    } else if let Some(name) = key.strip_prefix(b"engine-capacity-") {
        let capacity = number().and_then(|value| u32::try_from(value).ok());
        engine(client, name).extra_capacity += capacity.unwrap_or(1).saturating_sub(1);
    } else if let Some(name) = key.strip_prefix(b"engine-") {
        add_busy(engine(client, name), number());
    } else if let Some(name) = key.strip_prefix(b"cycles-") {
        add_busy(engine(client, name), number());
    } else if let Some(name) = key.strip_prefix(b"total-cycles-") {
        let counter = engine(client, name);
        counter.total_cycles = counter.total_cycles.max(number().unwrap_or(0));
    } else if let Some(region) = key.strip_prefix(b"resident-") {
        add_memory(client, region, RESIDENT_RANK, value);
    } else if let Some(region) = key.strip_prefix(b"memory-") {
        add_memory(client, region, LEGACY_RANK, value);
    } else if let Some(region) = key.strip_prefix(b"total-") {
        add_memory(client, region, TOTAL_RANK, value);
    }
}

fn engine<'c>(client: &'c mut DrmClient<'_>, name: &[u8]) -> &'c mut EngineCounter {
    &mut client.engines[EngineClass::from_driver_name(name).index()]
}

fn add_busy(counter: &mut EngineCounter, busy: Option<u64>) {
    if let Some(busy) = busy {
        counter.busy = counter.busy.saturating_add(busy);
        counter.names += 1;
    }
}

fn add_memory(client: &mut DrmClient<'_>, region: &[u8], rank: u8, value: &[u8]) {
    let Some(bytes) = memory_bytes(value) else {
        return;
    };
    match region_kind(region) {
        Some(RegionKind::Local) => client.local_memory.add(rank, bytes),
        Some(RegionKind::Shared) => client.shared_memory.add(rank, bytes),
        None => {}
    }
}

fn region_kind(region: &[u8]) -> Option<RegionKind> {
    if region.starts_with(b"vram") || region.starts_with(b"local") {
        Some(RegionKind::Local)
    } else if region == b"gtt" || region.starts_with(b"system") {
        Some(RegionKind::Shared)
    } else {
        None
    }
}

fn memory_bytes(value: &[u8]) -> Option<u64> {
    let mut fields = parse::fields(value);
    let amount = parse::parse_u64(fields.next()?)?;
    let unit = match fields.next() {
        None => 1,
        Some(b"KiB") => KIB,
        Some(b"MiB") => MIB,
        Some(b"GiB") => GIB,
        Some(_) => return None,
    };
    Some(amount.saturating_mul(unit))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A compositor's client of the Radeon 780M, from this machine.
    pub(crate) const AMDGPU_FDINFO: &str = "pos:\t0
flags:\t02104002
mnt_id:\t239
ino:\t543
drm-driver:\tamdgpu
drm-client-id:\t11
drm-pdev:\t0000:06:00.0
pasid:\t3
drm-total-cpu:\t0
drm-shared-cpu:\t0
drm-resident-cpu:\t0
drm-purgeable-cpu:\t0
drm-total-gtt:\t12300 KiB
drm-shared-gtt:\t0
drm-resident-gtt:\t65128 KiB
drm-purgeable-gtt:\t0
drm-total-vram:\t166996 KiB
drm-shared-vram:\t111928 KiB
drm-resident-vram:\t114168 KiB
drm-purgeable-vram:\t768 KiB
drm-total-gds:\t0
drm-resident-gds:\t0
drm-memory-vram:\t114168 KiB
drm-memory-gtt: \t65128 KiB
drm-memory-cpu: \t0 KiB
amd-evicted-vram:\t0 KiB
amd-requested-vram:\t166996 KiB
amd-requested-gtt:\t12300 KiB
drm-engine-gfx:\t46849625849 ns
drm-engine-compute:\t927155 ns
";

    const I915_FDINFO: &str = "drm-driver:\ti915
drm-pdev:\t0000:00:02.0
drm-client-id:\t7
drm-engine-render:\t9288864723 ns
drm-engine-copy:\t2035071108 ns
drm-engine-video:\t0 ns
drm-engine-capacity-video:\t2
drm-engine-video-enhance:\t0 ns
drm-total-system0:\t4 MiB
drm-resident-system0:\t3 MiB
";

    const XE_FDINFO: &str = "drm-driver:\txe
drm-pdev:\t0000:03:00.0
drm-client-id:\t42
drm-total-vram0:\t23992 KiB
drm-resident-vram0:\t23992 KiB
drm-total-gtt:\t4 KiB
drm-cycles-rcs:\t28257900
drm-total-cycles-rcs:\t7655183225
drm-cycles-vcs:\t0
drm-total-cycles-vcs:\t7655183225
drm-engine-capacity-vcs:\t2
";

    #[test]
    fn parses_amdgpu_client() {
        let client = parse(AMDGPU_FDINFO.as_bytes());
        assert_eq!(client.driver, Some(&b"amdgpu"[..]));
        assert_eq!(client.pdev, Some(&b"0000:06:00.0"[..]));
        assert_eq!(client.client_id, Some(11));
        let graphics = client.engines[EngineClass::Graphics.index()];
        assert_eq!(graphics.busy, 46_849_625_849);
        assert_eq!(graphics.capacity(), 1);
        assert!(!graphics.counts_cycles());
        assert_eq!(client.engines[EngineClass::Compute.index()].busy, 927_155);
        assert!(!client.engines[EngineClass::Copy.index()].is_present());
        assert_eq!(client.local_memory(), 114_168 * KIB);
        assert_eq!(client.shared_memory(), 65_128 * KIB);
    }

    #[test]
    fn parses_i915_capacity_and_system_memory() {
        let client = parse(I915_FDINFO.as_bytes());
        assert_eq!(client.engines[EngineClass::Video.index()].capacity(), 2);
        assert_eq!(
            client.engines[EngineClass::Copy.index()].busy,
            2_035_071_108
        );
        assert_eq!(client.shared_memory(), 3 * MIB);
        assert_eq!(client.local_memory(), 0);
    }

    #[test]
    fn parses_xe_cycles() {
        let client = parse(XE_FDINFO.as_bytes());
        let graphics = client.engines[EngineClass::Graphics.index()];
        assert!(graphics.counts_cycles());
        assert_eq!(graphics.busy, 28_257_900);
        assert_eq!(graphics.total_cycles, 7_655_183_225);
        assert_eq!(client.engines[EngineClass::Video.index()].capacity(), 2);
        assert_eq!(client.local_memory(), 23_992 * KIB);
        assert_eq!(client.shared_memory(), 4 * KIB);
    }

    #[test]
    fn ignores_non_drm_fdinfo() {
        let client = parse(b"pos:\t0\nflags:\t02100002\nmnt_id:\t38\nino:\t1191\n");
        assert!(!client.has_usage());
        assert_eq!(client.client_id, None);
    }
}
