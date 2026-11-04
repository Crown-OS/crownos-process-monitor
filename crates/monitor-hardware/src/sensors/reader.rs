//! Reading a chip's open channels, either on the sampler thread or, for
//! chips whose attributes block on an EC, an NVMe admin command or Wi-Fi
//! firmware, on a [`DeferredReader`] thread.

use std::time::{Duration, Instant};

use monitor_core::util::deferred::DeferredReader;
use monitor_core::{Celsius, Percent, Rpm};

use super::chip;
use super::discovery::{self, ChipReader};
use super::snapshot::{FanReading, TemperatureReading};
use crate::gpu::GpuPower;
use crate::util::attribute;

const PWM_FULL_SCALE: f32 = 255.0;
/// A chip whose channels take longer than this to read leaves the sampler
/// thread; fast chips (k10temp, amdgpu) read in a few microseconds.
pub(crate) const SLOW_CHIP_THRESHOLD: Duration = Duration::from_micros(300);
const THREAD_NAME: &str = "monitor-slow-sensors";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ChannelReading {
    Temperature {
        index: usize,
        value: Option<Celsius>,
    },
    Fan {
        index: usize,
        rpm: Option<Rpm>,
        duty: Option<Percent>,
    },
}

impl ChannelReading {
    pub(crate) fn apply(self, temperatures: &mut [TemperatureReading], fans: &mut [FanReading]) {
        match self {
            Self::Temperature { index, value } => {
                if let Some(reading) = temperatures.get_mut(index) {
                    reading.value = value;
                }
            }
            Self::Fan { index, rpm, duty } => {
                if let Some(reading) = fans.get_mut(index) {
                    reading.rpm = rpm;
                    reading.duty = duty;
                }
            }
        }
    }
}

impl ChipReader {
    /// Reads every channel; a suspended device reports nothing.
    pub(crate) fn read(&self, buffer: &mut Vec<u8>, mut emit: impl FnMut(ChannelReading)) {
        let awake = self.guard.state(buffer) == GpuPower::Active;
        for (input, index) in &self.temperatures {
            emit(ChannelReading::Temperature {
                index: *index,
                value: awake
                    .then(|| input.read_i64().ok())
                    .flatten()
                    .and_then(chip::plausible_temperature),
            });
        }
        for channel in &self.fans {
            emit(ChannelReading::Fan {
                index: channel.index,
                rpm: awake
                    .then(|| channel.rpm.read_u64().ok())
                    .flatten()
                    .and_then(discovery::plausible_rpm),
                duty: awake
                    .then(|| attribute::read_u64(channel.pwm.as_ref()))
                    .flatten()
                    .map(|pwm| Percent::clamped(pwm as f32 / PWM_FULL_SCALE * 100.0)),
            });
        }
    }

    /// Reads once into the readings and reports whether that took long.
    pub(crate) fn read_timed(
        &self,
        buffer: &mut Vec<u8>,
        temperatures: &mut [TemperatureReading],
        fans: &mut [FanReading],
    ) -> bool {
        let started = Instant::now();
        self.read(buffer, |reading| reading.apply(temperatures, fans));
        started.elapsed() > SLOW_CHIP_THRESHOLD
    }
}

/// Slow chips, read on request by their own thread.
pub(crate) type SlowChips = DeferredReader<Vec<ChannelReading>>;

pub(crate) fn spawn_slow_chips(chips: Vec<ChipReader>) -> Option<SlowChips> {
    if chips.is_empty() {
        return None;
    }
    DeferredReader::spawn(
        THREAD_NAME,
        (chips, Vec::new()),
        |(chips, buffer): &mut (Vec<ChipReader>, Vec<u8>), out: &mut Vec<ChannelReading>| {
            out.clear();
            for chip in chips.iter() {
                chip.read(buffer, |reading| out.push(reading));
            }
        },
    )
}
