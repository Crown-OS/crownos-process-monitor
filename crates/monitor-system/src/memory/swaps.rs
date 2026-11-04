use std::sync::Arc;

use monitor_core::Bytes;
use monitor_core::util::parse::{fields, lines, parse_i64, parse_u64};

use super::snapshot::{SwapDevice, SwapKind};

/// Refills `devices` from `/proc/swaps`, keeping names already interned.
pub(crate) fn parse_swaps(buffer: &[u8], devices: &mut Vec<SwapDevice>) {
    let mut count = 0;
    for line in lines(buffer).skip(1) {
        let mut columns = fields(line);
        let (Some(name), Some(kind), Some(size), Some(used), Some(priority)) = (
            columns.next(),
            columns.next(),
            columns.next().and_then(parse_u64),
            columns.next().and_then(parse_u64),
            columns.next().and_then(parse_i64),
        ) else {
            continue;
        };
        let name = String::from_utf8_lossy(name);
        let kind = if name.starts_with("/dev/zram") {
            SwapKind::Zram
        } else if kind == b"file" {
            SwapKind::File
        } else {
            SwapKind::Partition
        };
        if devices.len() <= count {
            devices.push(SwapDevice::default());
        }
        let device = &mut devices[count];
        if *device.name != *name {
            device.name = Arc::from(&*name);
        }
        device.kind = kind;
        device.size = Bytes::from_kib(size);
        device.used = Bytes::from_kib(used);
        device.priority = i32::try_from(priority).unwrap_or(0);
        count += 1;
    }
    devices.truncate(count);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_swaps() {
        let swaps = b"Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n\
/dev/zram0                              partition\t4194300\t\t2931164\t\t100\n\
/swapfile                               file\t\t8388604\t\t0\t\t-2\n";
        let mut devices = Vec::new();
        parse_swaps(swaps, &mut devices);
        assert_eq!(devices.len(), 2);
        assert_eq!(&*devices[0].name, "/dev/zram0");
        assert_eq!(devices[0].kind, SwapKind::Zram);
        assert_eq!(devices[0].used, Bytes::from_kib(2_931_164));
        assert_eq!(devices[1].kind, SwapKind::File);
        assert_eq!(devices[1].priority, -2);
        let interned = devices[0].name.clone();
        parse_swaps(swaps, &mut devices);
        assert!(Arc::ptr_eq(&interned, &devices[0].name));
        parse_swaps(b"Filename Type Size Used Priority\n", &mut devices);
        assert!(devices.is_empty());
    }
}
