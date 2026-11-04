use crate::{CgroupTraffic, DrmClient, EnergyZone, HelperError, ProcessUsage, SocketOwner};

#[zbus::proxy(
    interface = "org.crownos.ProcessMonitor1",
    default_service = "org.crownos.ProcessMonitor1",
    default_path = "/org/crownos/ProcessMonitor1"
)]
pub(crate) trait Helper {
    fn process_usage(&self, pids: &[u32]) -> Result<Vec<ProcessUsage>, HelperError>;

    fn socket_owners(&self) -> Result<Vec<SocketOwner>, HelperError>;

    fn drm_clients(&self) -> Result<Vec<DrmClient>, HelperError>;

    fn energy(&self) -> Result<Vec<EnergyZone>, HelperError>;

    fn network_accounting(&self) -> Result<Vec<CgroupTraffic>, HelperError>;

    fn set_fan(&self, fan_id: &str, percent: u8) -> Result<(), HelperError>;

    fn restore_fans(&self) -> Result<(), HelperError>;

    fn signal(&self, pid: u32, start_ticks: u64, signal: &str) -> Result<(), HelperError>;

    fn renice(&self, pid: u32, start_ticks: u64, nice: i32) -> Result<(), HelperError>;

    #[zbus(property)]
    fn version(&self) -> zbus::Result<u32>;

    #[zbus(property)]
    fn capabilities(&self) -> zbus::Result<Vec<String>>;
}
