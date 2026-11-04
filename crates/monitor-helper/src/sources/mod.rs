//! What the helper reads with root's view. Each reader sits behind its own
//! lock and runs on the blocking pool, never on the bus task.

mod descriptors;
mod energy;
mod network;
mod process_usage;

use std::sync::Mutex;

use descriptors::DescriptorReader;
use energy::EnergyReader;
use network::NetworkMeter;
pub(crate) use network::SUPPORTED as NETWORK_ACCOUNTING_SUPPORTED;
use process_usage::ProcessUsageReader;

use crate::util::lock::lock;
use crate::{CgroupTraffic, DrmClient, EnergyZone, HelperError, ProcessUsage, SocketOwner};

pub(crate) struct Sources {
    process_usage: Mutex<ProcessUsageReader>,
    descriptors: Mutex<DescriptorReader>,
    energy: Mutex<EnergyReader>,
    network: Mutex<NetworkMeter>,
}

impl Sources {
    pub(crate) fn open() -> Result<Self, HelperError> {
        Ok(Self {
            process_usage: Mutex::new(ProcessUsageReader::open()?),
            descriptors: Mutex::new(DescriptorReader::open()?),
            energy: Mutex::default(),
            network: Mutex::default(),
        })
    }

    pub(crate) fn process_usage(&self, pids: &[u32]) -> Result<Vec<ProcessUsage>, HelperError> {
        lock(&self.process_usage).sample(pids)
    }

    pub(crate) fn socket_owners(&self) -> Result<Vec<SocketOwner>, HelperError> {
        lock(&self.descriptors).socket_owners()
    }

    pub(crate) fn drm_clients(&self) -> Result<Vec<DrmClient>, HelperError> {
        lock(&self.descriptors).drm_clients()
    }

    pub(crate) fn energy(&self) -> Vec<EnergyZone> {
        lock(&self.energy).read()
    }

    pub(crate) fn network_accounting(&self) -> Result<Vec<CgroupTraffic>, HelperError> {
        lock(&self.network).read()
    }
}
