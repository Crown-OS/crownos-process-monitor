use monitor_engine::Snapshot;

#[cfg(test)]
use crate::app::live::HISTORY_CAPACITY;
use crate::app::live::LiveSnapshot;

/// Empty (all-gap) histories of full length, so pages built before the first
/// tick lay out at their final size.
pub fn prepare(live: &mut LiveSnapshot) {
    live.disk.prepare();
    live.network.prepare();
    live.cpu.prepare();
    live.memory.prepare();
    live.gpu.prepare();
}

/// Appends one tick to every history (a gap after a suspend) and refreshes
/// the current figures.
pub fn append(live: &mut LiveSnapshot, snapshot: &Snapshot) {
    if snapshot.elapsed.suspended {
        live.disk.push_gap();
        live.network.push_gap();
        live.cpu.push_gap();
        live.memory.push_gap();
        live.gpu.push_gap(snapshot);
    } else {
        push_rates(live, snapshot);
    }
    refresh_levels(live, snapshot);
    live.overview.append(snapshot);
}

fn push_rates(live: &mut LiveSnapshot, snapshot: &Snapshot) {
    live.cpu.push(snapshot);
    live.memory.push(snapshot);
    live.gpu.push(snapshot);
    live.disk.push(snapshot);
    live.network.push(snapshot);
}

fn refresh_levels(live: &mut LiveSnapshot, snapshot: &Snapshot) {
    live.cpu.refresh(snapshot);
    live.memory.refresh(snapshot);
    live.gpu.refresh(snapshot);
    live.disk.refresh(snapshot);
    live.network.refresh(snapshot);
}

#[cfg(test)]
mod tests {
    use monitor_core::{Elapsed, Percent};

    use super::*;

    #[test]
    fn a_suspended_tick_appends_a_gap() {
        let mut live = LiveSnapshot::default();
        prepare(&mut live);
        let mut snapshot = Snapshot::default();
        snapshot.cpu.total = Percent::new(40.0);
        append(&mut live, &snapshot);
        assert_eq!(live.cpu.total.last().copied(), Some(40.0));
        snapshot.elapsed = Elapsed {
            seconds: 1.0,
            suspended: true,
        };
        append(&mut live, &snapshot);
        assert!(live.cpu.total.last().is_some_and(|value| value.is_nan()));
        assert_eq!(live.cpu.total.len(), HISTORY_CAPACITY);
    }
}
