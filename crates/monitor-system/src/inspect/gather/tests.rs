use std::time::Duration;

use monitor_core::{Pid, ProcessKey, SourceError, SourceResult, Uid};

use super::procfile::process_file;
use super::*;
use crate::inspect::elf::ElfInfo;
use crate::process::parse_stat;

fn own_key() -> SourceResult<ProcessKey> {
    let pid = Pid::new(std::process::id());
    let text = std::fs::read(process_file(pid, "stat"))
        .map_err(|error| SourceError::io("own stat", error))?;
    let (_, stat) = parse_stat(&text).ok_or(SourceError::parse("own stat"))?;
    Ok(ProcessKey {
        pid,
        start_ticks: stat.start_ticks,
    })
}

#[test]
fn inspects_own_process() -> SourceResult<()> {
    let key = own_key()?;
    let details = inspect_with(
        key,
        &InspectContext {
            app: None,
            resolver: &AppResolver::new(DesktopIndex::default(), Uid::current()),
        },
    )?;
    assert_eq!(details.identity.key, key);
    assert_eq!(details.identity.uid, Uid::current());
    assert!(details.executable.path.is_some());
    assert_eq!(
        details.executable.elf.map(|elf| elf.architecture()),
        Some(ElfInfo::native_architecture())
    );
    assert!(details.resources.open_files.is_some_and(|count| count > 0));
    assert!(details.resources.proportional.is_some());
    assert!(
        details
            .environment
            .as_ref()
            .is_some_and(|environment| !environment.is_empty())
    );
    assert!(details.identity.running_for < Duration::from_secs(24 * 3600));
    assert!(!details.identity.app_name.is_empty());
    Ok(())
}

#[test]
fn rejects_reused_pid() -> SourceResult<()> {
    let mut key = own_key()?;
    key.start_ticks += 1;
    let error = inspect_with(
        key,
        &InspectContext {
            app: None,
            resolver: &AppResolver::new(DesktopIndex::default(), Uid::current()),
        },
    );
    assert!(error.is_err_and(|error| error.is_gone()));
    Ok(())
}

#[test]
fn inspects_init_with_partial_coverage() {
    let Ok(text) = std::fs::read("/proc/1/stat") else {
        return;
    };
    let Some((_, stat)) = parse_stat(&text) else {
        return;
    };
    let key = ProcessKey {
        pid: Pid::new(1),
        start_ticks: stat.start_ticks,
    };
    let Ok(details) = inspect_with(
        key,
        &InspectContext {
            app: None,
            resolver: &AppResolver::new(DesktopIndex::default(), Uid::current()),
        },
    ) else {
        return;
    };
    assert!(details.environment.is_none() || Uid::current() == Uid::ROOT);
    assert_eq!(details.identity.uid, Uid::ROOT);
}
