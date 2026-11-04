//! Marketing names from libdrm's `amdgpu.ids` (`device_id, revision_id, name`).

use std::path::Path;
use std::sync::Arc;

use monitor_core::util::parse;

pub(crate) fn lookup_file(path: &Path, device: u16, revision: u8) -> Option<Arc<str>> {
    let text = std::fs::read_to_string(path).ok()?;
    lookup(&text, device, revision).map(Arc::from)
}

pub(crate) fn lookup(text: &str, device: u16, revision: u8) -> Option<&str> {
    text.lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let mut columns = line.splitn(3, ',').map(str::trim);
            let device_id = parse::parse_hex_u32(columns.next()?.as_bytes())?;
            let revision_id = parse::parse_hex_u32(columns.next()?.as_bytes())?;
            let name = columns.next()?;
            (device_id == u32::from(device) && revision_id == u32::from(revision)).then_some(name)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "\
# List of AMDGPU IDs
#
# Syntax:
# device_id,\trevision_id,\tproduct_name        <-- single tab after comma

1.0.0
15BF,\tC1,\tAMD Radeon 780M Graphics
15BF,\tC2,\tAMD Radeon 780M Graphics
15BF,\tC3,\tAMD Radeon 760M Graphics
";

    #[test]
    fn matches_device_and_revision() {
        assert_eq!(
            lookup(FIXTURE, 0x15bf, 0xc2),
            Some("AMD Radeon 780M Graphics")
        );
        assert_eq!(
            lookup(FIXTURE, 0x15bf, 0xc3),
            Some("AMD Radeon 760M Graphics")
        );
        assert_eq!(lookup(FIXTURE, 0x15bf, 0x99), None);
    }
}
