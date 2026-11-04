//! Lazy lookup in `pci.ids`: the file is streamed line by line and only the
//! entry of the present vendor and device is kept.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use monitor_core::util::parse;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PciNames {
    pub(crate) vendor: Option<String>,
    pub(crate) device: Option<String>,
}

pub(crate) fn lookup_file(path: &Path, vendor: u16, device: u16) -> PciNames {
    File::open(path)
        .map(|file| lookup(BufReader::new(file), vendor, device))
        .unwrap_or_default()
}

pub(crate) fn lookup(mut reader: impl BufRead, vendor: u16, device: u16) -> PciNames {
    let mut names = PciNames::default();
    let mut line = Vec::with_capacity(128);
    let mut inside_vendor = false;
    while reader
        .read_until(b'\n', &mut line)
        .is_ok_and(|read| read > 0)
    {
        let text = line.trim_ascii_end();
        let is_entry = !text.is_empty() && !text.starts_with(b"#");
        if is_entry {
            match text.strip_prefix(b"\t") {
                None if inside_vendor => break,
                None => {
                    if let Some(name) = entry_name(text, vendor) {
                        names.vendor = Some(name);
                        inside_vendor = true;
                    }
                }
                Some(device_line) if inside_vendor && !device_line.starts_with(b"\t") => {
                    if let Some(name) = entry_name(device_line, device) {
                        names.device = Some(name);
                        break;
                    }
                }
                Some(_) => {}
            }
        }
        line.clear();
    }
    names
}

fn entry_name(line: &[u8], id: u16) -> Option<String> {
    let code = line.get(..4)?;
    (parse::parse_hex_u32(code)? == u32::from(id))
        .then(|| parse::as_str(&line[4..]).map(str::to_owned))
        .flatten()
}

/// The marketing part of a `pci.ids` device name: `AD107M [GeForce RTX 4050]`
/// becomes `GeForce RTX 4050`.
pub(crate) fn marketing_name(device: &str) -> &str {
    device
        .split_once('[')
        .and_then(|(_, rest)| rest.rsplit_once(']'))
        .map_or(device, |(inside, _)| inside)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "\
# comment
1002  Advanced Micro Devices, Inc. [AMD/ATI]
\t15bf  Phoenix1
\t\t17aa 3c92  Lenovo board
10de  NVIDIA Corporation
\t28e0  AD107M
\t28e1  AD107M [GeForce RTX 4050 Max-Q / Mobile]
\t28e3  AD107M [GeForce RTX 3050 A Laptop GPU]
10df  Emulex Corporation
\t28e1  Not this one
C 00  Unclassified device
";

    #[test]
    fn finds_device_under_vendor() {
        let names = lookup(FIXTURE.as_bytes(), 0x10de, 0x28e1);
        assert_eq!(names.vendor.as_deref(), Some("NVIDIA Corporation"));
        let device = names.device.unwrap_or_default();
        assert_eq!(marketing_name(&device), "GeForce RTX 4050 Max-Q / Mobile");
        let amd = lookup(FIXTURE.as_bytes(), 0x1002, 0x15bf);
        assert_eq!(amd.device.as_deref(), Some("Phoenix1"));
        assert_eq!(marketing_name("Phoenix1"), "Phoenix1");
    }

    #[test]
    fn stops_at_next_vendor() {
        let names = lookup(FIXTURE.as_bytes(), 0x1002, 0x28e1);
        assert!(names.vendor.is_some());
        assert_eq!(names.device, None);
    }

    #[test]
    fn reads_system_database() {
        let path = Path::new("/usr/share/hwdata/pci.ids");
        if path.exists() {
            assert!(lookup_file(path, 0x10de, 0x28e1).device.is_some());
        }
    }
}
