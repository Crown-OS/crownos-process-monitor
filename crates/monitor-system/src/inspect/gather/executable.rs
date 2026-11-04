use std::io::Read;

use monitor_core::Pid;

use super::procfile::{Restrictions, process_file};
use crate::inspect::details::Executable;
use crate::inspect::elf::{ElfInfo, HEADER_PREFIX};
use crate::inspect::security::{Emulation, binfmt_interpreter, emulation_hint};

const DELETED_SUFFIX: &str = " (deleted)";
const BINFMT_DIRECTORY: &str = "/proc/sys/fs/binfmt_misc";

pub(super) fn describe_executable(
    pid: Pid,
    link: Option<String>,
    command_line: &[String],
    restrictions: Restrictions,
) -> Executable {
    let Some(link) = link else {
        return Executable {
            coverage: restrictions.coverage(),
            ..Executable::default()
        };
    };
    let deleted = link.ends_with(DELETED_SUFFIX);
    let path = link
        .strip_suffix(DELETED_SUFFIX)
        .unwrap_or(&link)
        .to_owned();
    let elf = read_elf_header(pid);
    let foreign = elf.is_some_and(|elf| elf.architecture() != ElfInfo::native_architecture());
    let emulation = emulation_hint(&path, command_line).or_else(|| binfmt_handler(&path, foreign));
    Executable {
        path: Some(path),
        deleted,
        elf,
        emulation,
        coverage: restrictions.coverage(),
    }
}

fn read_elf_header(pid: Pid) -> Option<ElfInfo> {
    let mut header = [0_u8; HEADER_PREFIX];
    std::fs::File::open(process_file(pid, "exe"))
        .and_then(|mut file| file.read_exact(&mut header))
        .ok()?;
    ElfInfo::parse(&header)
}

/// A `binfmt_misc` entry whose interpreter is the executable, or any
/// enabled handler when the binary is for a foreign architecture.
fn binfmt_handler(executable: &str, foreign: bool) -> Option<Emulation> {
    let entries = std::fs::read_dir(BINFMT_DIRECTORY).ok()?;
    entries.flatten().find_map(|entry| {
        let handler = entry.file_name().to_string_lossy().into_owned();
        if matches!(handler.as_str(), "register" | "status") {
            return None;
        }
        let text = std::fs::read_to_string(entry.path()).ok()?;
        let interpreter = binfmt_interpreter(&text)?;
        (interpreter == executable || (foreign && text.starts_with("enabled"))).then(|| {
            Emulation::Binfmt {
                interpreter: interpreter.to_owned(),
                handler,
            }
        })
    })
}
