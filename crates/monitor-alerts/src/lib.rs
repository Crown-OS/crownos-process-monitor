//! Alert rules for the CrownOS process monitor: sustained CPU, memory
//! growth, disk and network bursts, heat, busy GPU and per-app custom
//! limits, evaluated on per-minute averages with cooldowns and quieting;
//! live strain state; and the Worth a Look feed model. Pure logic, no I/O.

mod alert;
mod cooldown;
mod detector;
mod feed;
mod input;
mod quiet;
mod rules;
mod strain;
mod thresholds;
mod tracker;

pub use alert::{Alert, AlertApp, AlertKind, AlertRecord, AlertUnit, CurrentState};
pub use detector::AlertDetector;
pub use feed::{EntryId, FeedDay, FeedEntry, WorthALook};
pub use input::{AlertInput, AppObservation, PressureLevel};
pub use quiet::{QuietRule, QuietScope, QuietUntil};
pub use strain::{Strain, StrainLimit, StrainState};
pub use thresholds::{AppLimit, CustomLimit, Thresholds};
