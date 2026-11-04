//! Manual fan control through hwmon `pwmN_enable` / `pwmN`. Writing needs
//! root, so as a normal user every call returns
//! [`FanControlError::PermissionDenied`]; the privileged helper calls this.

use std::io;
use std::path::{Path, PathBuf};

use monitor_core::Percent;
use monitor_core::util::sysfs;

use super::snapshot::FanId;

const MANUAL_MODE: u64 = 1;
const AUTOMATIC_MODE: u64 = 2;
const PWM_MIN: u64 = 0;
const PWM_MAX: u64 = 255;

#[derive(Debug, thiserror::Error)]
pub enum FanControlError {
    #[error("{0} has no PWM control")]
    NotControllable(PathBuf),
    #[error("writing {0} needs root; use the privileged helper")]
    PermissionDenied(PathBuf),
    #[error("failed to write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl FanId {
    fn attribute(&self, suffix: &str) -> PathBuf {
        self.chip_directory
            .join(format!("pwm{}{suffix}", self.channel))
    }
}

/// Remembers each fan's original mode so it can be put back on exit.
#[derive(Debug, Default)]
pub struct FanControl {
    saved_modes: Vec<(FanId, u64)>,
}

impl FanControl {
    /// Switches `fan` to manual mode at `duty` of its PWM range.
    pub fn set_manual(&mut self, fan: &FanId, duty: Percent) -> Result<(), FanControlError> {
        let enable = fan.attribute("_enable");
        let pwm = fan.attribute("");
        if !enable.exists() || !pwm.exists() {
            return Err(FanControlError::NotControllable(pwm));
        }
        if !self.saved_modes.iter().any(|(saved, _)| saved == fan) {
            let mode = sysfs::read_u64(&enable)
                .ok()
                .filter(|&mode| mode != MANUAL_MODE);
            self.saved_modes
                .push((fan.clone(), mode.unwrap_or(AUTOMATIC_MODE)));
        }
        let (low, high) = pwm_range(fan);
        write(&enable, MANUAL_MODE)?;
        write(&pwm, scale(duty, low, high))
    }

    /// Hands `fan` back to the mode it had before [`Self::set_manual`], or
    /// to automatic control.
    pub fn restore_automatic(&mut self, fan: &FanId) -> Result<(), FanControlError> {
        let position = self.saved_modes.iter().position(|(saved, _)| saved == fan);
        let mode = position.map_or(AUTOMATIC_MODE, |index| {
            self.saved_modes.swap_remove(index).1
        });
        write(&fan.attribute("_enable"), mode)
    }

    /// Restores every fan this controller changed.
    pub fn restore_all(&mut self) -> Result<(), FanControlError> {
        let mut saved = std::mem::take(&mut self.saved_modes).into_iter();
        let restored = saved
            .by_ref()
            .try_for_each(|(fan, mode)| write(&fan.attribute("_enable"), mode));
        self.saved_modes.extend(saved);
        restored
    }
}

fn pwm_range(fan: &FanId) -> (u64, u64) {
    let low = sysfs::read_u64(fan.attribute("_min")).unwrap_or(PWM_MIN);
    let high = sysfs::read_u64(fan.attribute("_max")).unwrap_or(PWM_MAX);
    if high > low {
        (low, high)
    } else {
        (PWM_MIN, PWM_MAX)
    }
}

fn scale(duty: Percent, low: u64, high: u64) -> u64 {
    low + ((high - low) as f64 * duty.as_ratio()).round() as u64
}

fn write(path: &Path, value: u64) -> Result<(), FanControlError> {
    std::fs::write(path, value.to_string()).map_err(|source| match source.kind() {
        io::ErrorKind::PermissionDenied => FanControlError::PermissionDenied(path.to_owned()),
        _ => FanControlError::Io {
            path: path.to_owned(),
            source,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_duty_into_pwm_range() {
        assert_eq!(scale(Percent::new(0.0), 0, 255), 0);
        assert_eq!(scale(Percent::new(50.0), 0, 255), 128);
        assert_eq!(scale(Percent::new(100.0), 0, 255), 255);
        assert_eq!(scale(Percent::new(50.0), 100, 200), 150);
        assert_eq!(scale(Percent::new(140.0), 0, 255), 255);
    }

    #[test]
    fn rejects_fans_without_pwm() {
        let fan = FanId {
            chip_directory: Path::new("/nonexistent/hwmon").into(),
            channel: 1,
        };
        let mut control = FanControl::default();
        assert!(matches!(
            control.set_manual(&fan, Percent::new(40.0)),
            Err(FanControlError::NotControllable(_))
        ));
    }
}
