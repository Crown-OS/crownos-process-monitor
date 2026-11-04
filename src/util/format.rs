//! Human-readable figures. Every `write_*` appends to a caller-owned buffer so hot paths can
//! reuse one `String`; the plain functions are conveniences that allocate.

use std::fmt::Write;

use monitor_core::Celsius;
use serde::{Deserialize, Serialize};

const NOT_AVAILABLE: &str = "–";
const BINARY_STEP: f64 = 1024.0;
const DECIMAL_STEP: f64 = 1000.0;
const UNIT_ROLLOVER: f64 = 1000.0;
const BYTE_UNITS: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];
const BIT_RATE_UNITS: [&str; 5] = ["bps", "kbps", "Mbps", "Gbps", "Tbps"];
const BITS_PER_BYTE: f64 = 8.0;
const FIRST_FRACTIONAL_UNIT: usize = 2;
const TWO_DECIMALS_BELOW: f64 = 100.0;
const ONE_DECIMAL_BELOW: f64 = 100.0;
const PERCENT_DECIMAL_BELOW: f32 = 10.0;
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3_600;
const SECONDS_PER_DAY: u64 = 86_400;
const MILLIWATTS_PER_WATT: f32 = 1000.0;
const WATT_DECIMAL_BELOW: f32 = 10.0;
const MEGAHERTZ_PER_GIGAHERTZ: f32 = 1000.0;
const DIGIT_GROUP: usize = 3;

/// How network and disk rates are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RateUnit {
    /// Binary bytes per second: "1.3 MB/s".
    #[default]
    Bytes,
    /// Decimal bits per second: "10.4 Mbps".
    Bits,
}

/// Which temperature scale readings use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TemperatureScale {
    #[default]
    Celsius,
    Fahrenheit,
}

fn scaled(value: f64, step: f64, unit_count: usize) -> (f64, usize) {
    let mut scaled = value;
    let mut unit = 0;
    while scaled >= UNIT_ROLLOVER && unit + 1 < unit_count {
        scaled /= step;
        unit += 1;
    }
    (scaled, unit)
}

/// "53.08 GB", "842 MB", "12 kB": binary steps, two decimals below 100 from MB up.
pub fn write_bytes(out: &mut String, bytes: u64) {
    let (value, unit) = scaled(bytes as f64, BINARY_STEP, BYTE_UNITS.len());
    let decimals = if unit >= FIRST_FRACTIONAL_UNIT && value < TWO_DECIMALS_BELOW {
        2
    } else {
        0
    };
    let _ = write!(out, "{value:.decimals$} {}", BYTE_UNITS[unit]);
}

pub fn bytes(bytes: u64) -> String {
    let mut out = String::new();
    write_bytes(&mut out, bytes);
    out
}

/// "1.3 MB/s" in bytes mode, "10.4 Mbps" in bits mode.
pub fn write_rate(out: &mut String, bytes_per_second: f64, unit: RateUnit) {
    if !bytes_per_second.is_finite() {
        out.push_str(NOT_AVAILABLE);
        return;
    }
    let bytes_per_second = bytes_per_second.max(0.0);
    let (value, unit_label, first_fractional) = match unit {
        RateUnit::Bytes => {
            let (value, index) = scaled(bytes_per_second, BINARY_STEP, BYTE_UNITS.len());
            (value, index, FIRST_FRACTIONAL_UNIT)
        }
        RateUnit::Bits => {
            let bits = bytes_per_second * BITS_PER_BYTE;
            let (value, index) = scaled(bits, DECIMAL_STEP, BIT_RATE_UNITS.len());
            (value, index, 1)
        }
    };
    let decimals = if unit_label >= first_fractional && value < ONE_DECIMAL_BELOW {
        1
    } else {
        0
    };
    let _ = match unit {
        RateUnit::Bytes => write!(out, "{value:.decimals$} {}/s", BYTE_UNITS[unit_label]),
        RateUnit::Bits => write!(out, "{value:.decimals$} {}", BIT_RATE_UNITS[unit_label]),
    };
}

pub fn rate(bytes_per_second: f64, unit: RateUnit) -> String {
    let mut out = String::new();
    write_rate(&mut out, bytes_per_second, unit);
    out
}

/// "21%", with one decimal for small non-zero values: "4.5%".
pub fn write_percent(out: &mut String, percent: f32) {
    if !percent.is_finite() {
        out.push_str(NOT_AVAILABLE);
        return;
    }
    let decimals = if percent > 0.0 && percent < PERCENT_DECIMAL_BELOW {
        1
    } else {
        0
    };
    let _ = write!(out, "{percent:.decimals$}%");
}

pub fn percent(percent: f32) -> String {
    let mut out = String::new();
    write_percent(&mut out, percent);
    out
}

/// "45 s", "45 min", "6h 12m", "3d 4h".
pub fn write_duration(out: &mut String, seconds: u64) {
    let _ = match seconds {
        seconds if seconds < SECONDS_PER_MINUTE => write!(out, "{seconds} s"),
        seconds if seconds < SECONDS_PER_HOUR => {
            write!(out, "{} min", seconds / SECONDS_PER_MINUTE)
        }
        seconds if seconds < SECONDS_PER_DAY => write!(
            out,
            "{}h {}m",
            seconds / SECONDS_PER_HOUR,
            seconds % SECONDS_PER_HOUR / SECONDS_PER_MINUTE
        ),
        seconds => write!(
            out,
            "{}d {}h",
            seconds / SECONDS_PER_DAY,
            seconds % SECONDS_PER_DAY / SECONDS_PER_HOUR
        ),
    };
}

pub fn duration(seconds: u64) -> String {
    let mut out = String::new();
    write_duration(&mut out, seconds);
    out
}

/// "62 °C" or "144 °F".
pub fn write_temperature(out: &mut String, celsius: f32, scale: TemperatureScale) {
    if !celsius.is_finite() {
        out.push_str(NOT_AVAILABLE);
        return;
    }
    let _ = match scale {
        TemperatureScale::Celsius => write!(out, "{celsius:.0} °C"),
        TemperatureScale::Fahrenheit => write!(out, "{:.0} °F", Celsius::new(celsius).fahrenheit()),
    };
}

pub fn temperature(celsius: f32, scale: TemperatureScale) -> String {
    let mut out = String::new();
    write_temperature(&mut out, celsius, scale);
    out
}

/// "850 mW", "4.2 W", "42 W".
pub fn write_power(out: &mut String, watts: f32) {
    if !watts.is_finite() {
        out.push_str(NOT_AVAILABLE);
        return;
    }
    let _ = match watts {
        watts if watts < 1.0 => write!(out, "{:.0} mW", watts * MILLIWATTS_PER_WATT),
        watts if watts < WATT_DECIMAL_BELOW => write!(out, "{watts:.1} W"),
        watts => write!(out, "{watts:.0} W"),
    };
}

pub fn power(watts: f32) -> String {
    let mut out = String::new();
    write_power(&mut out, watts);
    out
}

/// "800 MHz", "3.42 GHz".
pub fn write_frequency(out: &mut String, megahertz: f32) {
    if !megahertz.is_finite() {
        out.push_str(NOT_AVAILABLE);
        return;
    }
    let _ = if megahertz < MEGAHERTZ_PER_GIGAHERTZ {
        write!(out, "{megahertz:.0} MHz")
    } else {
        write!(out, "{:.2} GHz", megahertz / MEGAHERTZ_PER_GIGAHERTZ)
    };
}

pub fn frequency(megahertz: f32) -> String {
    let mut out = String::new();
    write_frequency(&mut out, megahertz);
    out
}

/// "1,234,567".
pub fn write_count(out: &mut String, value: u64) {
    let mut digits = [0_u8; 20];
    let mut remaining = value;
    let mut length = 0;
    loop {
        digits[length] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        length += 1;
        if remaining == 0 {
            break;
        }
    }
    for (position, digit) in digits[..length].iter().rev().enumerate() {
        if position > 0 && (length - position) % DIGIT_GROUP == 0 {
            out.push(',');
        }
        out.push(char::from(*digit));
    }
}

pub fn count(value: u64) -> String {
    let mut out = String::new();
    write_count(&mut out, value);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    #[test]
    fn bytes_use_binary_steps_and_adaptive_precision() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(12 * KIB), "12 kB");
        assert_eq!(bytes(842 * MIB), "842 MB");
        assert_eq!(bytes(53 * GIB + 82 * MIB), "53.08 GB");
        assert_eq!(bytes(1010 * MIB), "0.99 GB");
    }

    #[test]
    fn rates_follow_the_unit_setting() {
        assert_eq!(rate(1.3 * MIB as f64, RateUnit::Bytes), "1.3 MB/s");
        assert_eq!(rate(842.0 * KIB as f64, RateUnit::Bytes), "842 kB/s");
        assert_eq!(rate(1_300_000.0, RateUnit::Bits), "10.4 Mbps");
        assert_eq!(rate(100.0, RateUnit::Bits), "800 bps");
        assert_eq!(rate(f64::NAN, RateUnit::Bytes), "–");
    }

    #[test]
    fn percents_keep_a_decimal_only_when_small() {
        assert_eq!(percent(21.4), "21%");
        assert_eq!(percent(4.52), "4.5%");
        assert_eq!(percent(0.0), "0%");
        assert_eq!(percent(f32::NAN), "–");
    }

    #[test]
    fn durations_pick_the_two_largest_units() {
        assert_eq!(duration(12), "12 s");
        assert_eq!(duration(45 * 60), "45 min");
        assert_eq!(duration(6 * 3600 + 12 * 60), "6h 12m");
        assert_eq!(duration(3 * 86_400 + 4 * 3600), "3d 4h");
    }

    #[test]
    fn temperatures_convert_to_fahrenheit() {
        assert_eq!(temperature(62.0, TemperatureScale::Celsius), "62 °C");
        assert_eq!(temperature(62.0, TemperatureScale::Fahrenheit), "144 °F");
    }

    #[test]
    fn power_and_frequency_switch_units() {
        assert_eq!(power(0.85), "850 mW");
        assert_eq!(power(4.24), "4.2 W");
        assert_eq!(power(42.4), "42 W");
        assert_eq!(frequency(800.0), "800 MHz");
        assert_eq!(frequency(3420.0), "3.42 GHz");
    }

    #[test]
    fn counts_group_thousands() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1_000), "1,000");
        assert_eq!(count(1_234_567), "1,234,567");
    }

    #[test]
    fn writers_append_to_the_buffer() {
        let mut out = String::from("Used ");
        write_bytes(&mut out, 12 * KIB);
        out.push_str(" · ");
        write_count(&mut out, 4096);
        assert_eq!(out, "Used 12 kB · 4,096");
    }
}
