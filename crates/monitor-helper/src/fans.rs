//! Manual fan speeds, owned by the client that set them. When that client
//! leaves the bus, or the helper exits, its fans go back to automatic.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use monitor_core::Percent;
use monitor_hardware::{FanControl, FanControlError, FanId};

use crate::HelperError;

const HWMON_CLASS: &str = "/sys/class/hwmon";
const PWM_PREFIX: &str = "pwm";
const MAX_PERCENT: u8 = 100;

/// The bus form of a fan: the path of its `pwmN` file,
/// e.g. `/sys/devices/platform/legion/hwmon/hwmon5/pwm1`.
pub fn fan_id_string(fan: &FanId) -> String {
    format!(
        "{}/{PWM_PREFIX}{}",
        fan.chip_directory.display(),
        fan.channel
    )
}

/// Splits a fan id into its chip directory and PWM channel, without
/// touching the file system.
fn parse_fan_id(fan_id: &str) -> Result<(&Path, u8), HelperError> {
    let invalid = || HelperError::invalid(format!("{fan_id:?} is not a hwmon pwm file"));
    let path = Path::new(fan_id);
    if !path.is_absolute() {
        return Err(invalid());
    }
    let channel = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(PWM_PREFIX))
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|digits| digits.parse::<u8>().ok())
        .filter(|&channel| channel > 0)
        .ok_or_else(invalid)?;
    let chip = path.parent().ok_or_else(invalid)?;
    Ok((chip, channel))
}

/// Accepts only PWM channels of a chip registered under
/// `/sys/class/hwmon`, so a client can never steer writes elsewhere.
fn resolve_fan(fan_id: &str) -> Result<FanId, HelperError> {
    let (chip, channel) = parse_fan_id(fan_id)?;
    let unknown = || HelperError::invalid(format!("{fan_id:?} is not a hwmon fan"));
    let chip = std::fs::canonicalize(chip).map_err(|_| unknown())?;
    if !hwmon_chips().any(|known| known == chip) {
        return Err(unknown());
    }
    Ok(FanId {
        chip_directory: Arc::from(chip),
        channel,
    })
}

fn hwmon_chips() -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(HWMON_CLASS)
        .into_iter()
        .flatten()
        .filter_map(|entry| std::fs::canonicalize(entry.ok()?.path()).ok())
}

fn validate_percent(percent: u8) -> Result<Percent, HelperError> {
    if percent > MAX_PERCENT {
        return Err(HelperError::invalid(format!(
            "fan duty {percent} % is above 100 %"
        )));
    }
    Ok(Percent::new(f32::from(percent)))
}

/// Which client holds each manually set fan. The last client to set a fan
/// owns it.
#[derive(Debug, Default)]
pub(crate) struct FanOwnership {
    owners: Vec<(FanId, String)>,
}

impl FanOwnership {
    pub fn claim(&mut self, fan: FanId, owner: &str) {
        match self.owners.iter_mut().find(|(held, _)| *held == fan) {
            Some((_, holder)) if holder != owner => owner.clone_into(holder),
            Some(_) => {}
            None => self.owners.push((fan, owner.to_owned())),
        }
    }

    #[cfg(test)]
    fn owner_of(&self, fan: &FanId) -> Option<&str> {
        self.owners
            .iter()
            .find_map(|(held, owner)| (held == fan).then_some(owner.as_str()))
    }

    /// Forgets and returns the fans `owner` held.
    pub fn release_owner(&mut self, owner: &str) -> Vec<FanId> {
        self.owners
            .extract_if(.., |(_, holder)| holder == owner)
            .map(|(fan, _)| fan)
            .collect()
    }

    pub fn release_all(&mut self) -> Vec<FanId> {
        self.owners.drain(..).map(|(fan, _)| fan).collect()
    }
}

/// The helper's fan control: hwmon writes plus who asked for them.
#[derive(Debug, Default)]
pub(crate) struct FanBoard {
    control: FanControl,
    ownership: FanOwnership,
}

impl FanBoard {
    pub fn set(&mut self, fan_id: &str, percent: u8, owner: &str) -> Result<(), HelperError> {
        let duty = validate_percent(percent)?;
        let fan = resolve_fan(fan_id)?;
        self.control
            .set_manual(&fan, duty)
            .map_err(fan_control_error)?;
        self.ownership.claim(fan, owner);
        Ok(())
    }

    pub fn restore_owned_by(&mut self, owner: &str) -> Result<(), HelperError> {
        let fans = self.ownership.release_owner(owner);
        self.restore(&fans)
    }

    pub fn restore_all(&mut self) -> Result<(), HelperError> {
        let fans = self.ownership.release_all();
        self.restore(&fans)?;
        self.control.restore_all().map_err(fan_control_error)
    }

    /// Restores every fan even when one fails, reporting the first failure.
    fn restore(&mut self, fans: &[FanId]) -> Result<(), HelperError> {
        let failures: Vec<FanControlError> = fans
            .iter()
            .filter_map(|fan| self.control.restore_automatic(fan).err())
            .collect();
        failures
            .into_iter()
            .next()
            .map_or(Ok(()), |failure| Err(fan_control_error(failure)))
    }
}

fn fan_control_error(error: FanControlError) -> HelperError {
    match error {
        FanControlError::NotControllable(_) => HelperError::NotSupported(error.to_string()),
        FanControlError::PermissionDenied(_) | FanControlError::Io { .. } => {
            HelperError::failed(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fan(chip: &str, channel: u8) -> FanId {
        FanId {
            chip_directory: Arc::from(Path::new(chip)),
            channel,
        }
    }

    #[test]
    fn parses_fan_ids() {
        let parsed = parse_fan_id("/sys/class/hwmon/hwmon5/pwm2");
        assert!(matches!(parsed, Ok((chip, 2)) if chip == Path::new("/sys/class/hwmon/hwmon5")));
        for bad in [
            "hwmon5/pwm1",
            "/sys/class/hwmon/hwmon5/pwm",
            "/sys/class/hwmon/hwmon5/pwm0",
            "/sys/class/hwmon/hwmon5/pwm1_enable",
            "/sys/class/hwmon/hwmon5/pwm+1",
            "/sys/class/hwmon/hwmon5/pwm999",
            "/sys/class/hwmon/hwmon5/fan1_input",
            "",
        ] {
            assert!(
                matches!(parse_fan_id(bad), Err(HelperError::InvalidArgument(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn round_trips_fan_ids() {
        let id = fan("/sys/devices/platform/legion/hwmon/hwmon5", 1);
        let text = fan_id_string(&id);
        assert_eq!(text, "/sys/devices/platform/legion/hwmon/hwmon5/pwm1");
        assert!(matches!(parse_fan_id(&text), Ok((chip, 1)) if *chip == *id.chip_directory));
    }

    #[test]
    fn resolves_only_hwmon_chips() {
        assert!(resolve_fan("/etc/pwm1").is_err());
        assert!(resolve_fan("/sys/class/hwmon/../../../etc/pwm1").is_err());
        assert!(resolve_fan("/nonexistent/pwm1").is_err());
        let Some(chip) = hwmon_chips().next() else {
            return;
        };
        let id = format!("{}/pwm1", chip.display());
        assert!(resolve_fan(&id).is_ok_and(|fan| *fan.chip_directory == *chip));
        let through_class = format!(
            "{HWMON_CLASS}/{}/pwm3",
            chip.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
        );
        assert!(resolve_fan(&through_class).is_ok_and(|fan| fan.channel == 3));
    }

    #[test]
    fn validates_duty() {
        assert!(validate_percent(0).is_ok());
        assert!(validate_percent(100).is_ok_and(|duty| duty.get() == 100.0));
        assert!(matches!(
            validate_percent(101),
            Err(HelperError::InvalidArgument(_))
        ));
    }

    #[test]
    fn tracks_fan_owners() {
        let mut ownership = FanOwnership::default();
        let cpu = fan("/sys/class/hwmon/hwmon5", 1);
        let gpu = fan("/sys/class/hwmon/hwmon5", 2);
        ownership.claim(cpu.clone(), ":1.10");
        ownership.claim(gpu.clone(), ":1.10");
        ownership.claim(gpu.clone(), ":1.20");
        assert_eq!(ownership.owner_of(&cpu), Some(":1.10"));
        assert_eq!(ownership.owner_of(&gpu), Some(":1.20"), "last writer owns");

        assert_eq!(ownership.release_owner(":1.10"), vec![cpu.clone()]);
        assert_eq!(ownership.owner_of(&cpu), None);
        assert!(ownership.release_owner(":1.10").is_empty());

        ownership.claim(cpu.clone(), ":1.30");
        let mut all = ownership.release_all();
        all.sort_by_key(|fan| fan.channel);
        assert_eq!(all, vec![cpu, gpu]);
        assert!(ownership.release_all().is_empty());
    }

    #[test]
    fn rejects_bad_requests_before_touching_hardware() {
        let mut board = FanBoard::default();
        assert!(matches!(
            board.set("/sys/class/hwmon/hwmon0/pwm1", 150, ":1.1"),
            Err(HelperError::InvalidArgument(_))
        ));
        assert!(matches!(
            board.set("/tmp/pwm1", 50, ":1.1"),
            Err(HelperError::InvalidArgument(_))
        ));
        assert!(board.restore_owned_by(":1.1").is_ok());
        assert!(board.restore_all().is_ok());
    }
}
