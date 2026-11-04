#[cfg(feature = "ebpf")]
pub(crate) use crate::bpf::CgroupTrafficMeter as NetworkMeter;
#[cfg(not(feature = "ebpf"))]
pub(crate) use unsupported::NetworkMeter;

/// Whether this build can serve `NetworkAccounting`.
pub(crate) const SUPPORTED: bool = cfg!(feature = "ebpf");

#[cfg(not(feature = "ebpf"))]
mod unsupported {
    use crate::{CgroupTraffic, HelperError};

    /// Stands in for the eBPF meter in builds without the `ebpf` feature.
    #[derive(Default)]
    pub(crate) struct NetworkMeter;

    impl NetworkMeter {
        pub(crate) fn read(&mut self) -> Result<Vec<CgroupTraffic>, HelperError> {
            Err(HelperError::NotSupported(
                "per-cgroup network accounting needs a helper built with `--features ebpf`"
                    .to_owned(),
            ))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reports_missing_feature() {
            const { assert!(!super::super::SUPPORTED) };
            assert!(matches!(
                NetworkMeter.read(),
                Err(HelperError::NotSupported(_))
            ));
        }
    }
}
