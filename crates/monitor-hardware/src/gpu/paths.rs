use std::path::PathBuf;

const SYSFS: &str = "/sys";
const PCI_IDS: &str = "/usr/share/hwdata/pci.ids";
const AMDGPU_IDS: &str = "/usr/share/libdrm/amdgpu.ids";
const KERNEL_RELEASE: &str = "/proc/sys/kernel/osrelease";

/// Filesystem roots the GPU collector reads, replaceable in tests.
#[derive(Clone, Debug)]
pub(crate) struct SystemPaths {
    pub(crate) sysfs: PathBuf,
    pub(crate) pci_ids: PathBuf,
    pub(crate) amdgpu_ids: PathBuf,
    pub(crate) kernel_release: PathBuf,
}

impl Default for SystemPaths {
    fn default() -> Self {
        Self {
            sysfs: PathBuf::from(SYSFS),
            pci_ids: PathBuf::from(PCI_IDS),
            amdgpu_ids: PathBuf::from(AMDGPU_IDS),
            kernel_release: PathBuf::from(KERNEL_RELEASE),
        }
    }
}

impl SystemPaths {
    pub(crate) fn drm(&self) -> PathBuf {
        self.sysfs.join("class/drm")
    }

    pub(crate) fn accel(&self) -> PathBuf {
        self.sysfs.join("class/accel")
    }

    pub(crate) fn module_version(&self, driver: &str) -> PathBuf {
        self.sysfs.join("module").join(driver).join("version")
    }
}
