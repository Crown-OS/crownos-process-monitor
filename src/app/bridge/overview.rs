//! The Overview's own live series: power draw, the hottest sensor and each accelerator.
//! Every other figure it shows is read from the shared tick.

use monitor_core::DeviceKey;
use monitor_engine::Snapshot;
use monitor_engine::hardware::AcceleratorSnapshot;

use crate::app::live::{HISTORY_CAPACITY, push_sample};

#[derive(Debug, Default)]
pub struct OverviewReadings {
    /// Watts.
    pub power_draw: Vec<f32>,
    /// °C of the hottest sensor.
    pub hottest: Vec<f32>,
    /// Busy percent per accelerator, in the snapshot's order.
    pub accelerators: Vec<AcceleratorHistory>,
}

#[derive(Debug)]
pub struct AcceleratorHistory {
    pub key: DeviceKey,
    pub busy: Vec<f32>,
}

impl OverviewReadings {
    /// Appends one tick (a gap after a suspend); the histories are sized on first use.
    pub fn append(&mut self, snapshot: &Snapshot) {
        self.follow_accelerators(&snapshot.gpu.accelerators);
        if snapshot.elapsed.suspended {
            self.histories()
                .for_each(|history| push_sample(history, f32::NAN));
            return;
        }
        let draw = snapshot
            .system_draw
            .or(snapshot.power.system_draw)
            .map_or(f32::NAN, |draw| draw.watts.get());
        push_sample(sized(&mut self.power_draw), draw);
        let hottest = snapshot
            .sensors
            .hottest_sensor()
            .and_then(|reading| reading.value)
            .map_or(f32::NAN, |celsius| celsius.get());
        push_sample(sized(&mut self.hottest), hottest);
        for (history, accelerator) in self.accelerators.iter_mut().zip(&snapshot.gpu.accelerators) {
            let busy = accelerator.busy.map_or(f32::NAN, |busy| busy.get());
            push_sample(&mut history.busy, busy);
        }
    }

    fn follow_accelerators(&mut self, accelerators: &[AcceleratorSnapshot]) {
        let unchanged = self.accelerators.len() == accelerators.len()
            && self
                .accelerators
                .iter()
                .zip(accelerators)
                .all(|(history, accelerator)| history.key == accelerator.key);
        if !unchanged {
            self.accelerators = accelerators
                .iter()
                .map(|accelerator| AcceleratorHistory {
                    key: accelerator.key.clone(),
                    busy: vec![f32::NAN; HISTORY_CAPACITY],
                })
                .collect();
        }
    }

    fn histories(&mut self) -> impl Iterator<Item = &mut Vec<f32>> {
        [sized(&mut self.power_draw), sized(&mut self.hottest)]
            .into_iter()
            .chain(
                self.accelerators
                    .iter_mut()
                    .map(|history| &mut history.busy),
            )
    }
}

fn sized(history: &mut Vec<f32>) -> &mut Vec<f32> {
    if history.len() != HISTORY_CAPACITY {
        history.clear();
        history.resize(HISTORY_CAPACITY, f32::NAN);
    }
    history
}

#[cfg(test)]
mod tests {
    use monitor_core::{Coverage, Elapsed, Watts};
    use monitor_engine::hardware::SystemDraw;

    use super::*;

    #[test]
    fn draw_and_heat_are_appended_and_suspends_leave_gaps() {
        let mut readings = OverviewReadings::default();
        let mut snapshot = Snapshot {
            system_draw: Some(SystemDraw {
                watts: Watts::new(12.5),
                coverage: Coverage::Full,
            }),
            ..Snapshot::default()
        };
        readings.append(&snapshot);
        assert_eq!(readings.power_draw.len(), HISTORY_CAPACITY);
        assert_eq!(readings.power_draw.last().copied(), Some(12.5));
        assert!(readings.hottest.last().is_some_and(|value| value.is_nan()));
        snapshot.elapsed = Elapsed {
            seconds: 1.0,
            suspended: true,
        };
        readings.append(&snapshot);
        assert!(
            readings
                .power_draw
                .last()
                .is_some_and(|value| value.is_nan())
        );
    }
}
