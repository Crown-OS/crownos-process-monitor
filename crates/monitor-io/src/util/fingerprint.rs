const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Order-sensitive FNV-1a fingerprint of a sequence of names, used to notice
/// hotplug without allocating a set per tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fingerprint(u64);

impl Default for Fingerprint {
    fn default() -> Self {
        Self(FNV_OFFSET)
    }
}

impl Fingerprint {
    pub(crate) fn add(&mut self, name: &[u8]) {
        for &byte in name.iter().chain(std::iter::once(&0)) {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn differs_when_names_change() {
        let mut first = Fingerprint::default();
        first.add(b"nvme0n1");
        first.add(b"nvme1n1");
        let mut second = Fingerprint::default();
        second.add(b"nvme0n1");
        assert_ne!(first, second);
        second.add(b"nvme1n1");
        assert_eq!(first, second);
    }
}
