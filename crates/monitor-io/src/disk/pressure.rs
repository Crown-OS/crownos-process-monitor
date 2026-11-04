//! `/proc/pressure/io`: the share of time tasks stalled on I/O.

use monitor_core::Percent;
use monitor_core::util::parse;

const SOME: &[u8] = b"some";
const FULL: &[u8] = b"full";
const AVG10: &[u8] = b"avg10=";
const AVG60: &[u8] = b"avg60=";

/// PSI averages. `some`: at least one task waited; `full`: all non-idle
/// tasks waited at once.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IoPressure {
    pub some_avg10: Percent,
    pub some_avg60: Percent,
    pub full_avg10: Percent,
    pub full_avg60: Percent,
}

pub(crate) fn parse_pressure(text: &[u8]) -> Option<IoPressure> {
    let mut pressure = IoPressure::default();
    let mut seen_some = false;
    for line in parse::lines(text) {
        let mut columns = parse::fields(line);
        let (avg10, avg60) = match columns.next()? {
            SOME => {
                seen_some = true;
                (&mut pressure.some_avg10, &mut pressure.some_avg60)
            }
            FULL => (&mut pressure.full_avg10, &mut pressure.full_avg60),
            _ => continue,
        };
        for column in columns {
            if let Some(value) = column.strip_prefix(AVG10) {
                *avg10 = Percent::clamped(parse::parse_f64(value)? as f32);
            } else if let Some(value) = column.strip_prefix(AVG60) {
                *avg60 = Percent::clamped(parse::parse_f64(value)? as f32);
            }
        }
    }
    seen_some.then_some(pressure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_psi_lines() {
        let text = b"some avg10=0.06 avg60=0.18 avg300=0.39 total=10788516
full avg10=0.05 avg60=0.17 avg300=0.35 total=10088811
";
        let Some(pressure) = parse_pressure(text) else {
            panic!("PSI text must parse");
        };
        assert_eq!(pressure.some_avg10.get(), 0.06);
        assert_eq!(pressure.some_avg60.get(), 0.18);
        assert_eq!(pressure.full_avg10.get(), 0.05);
        assert_eq!(pressure.full_avg60.get(), 0.17);
        assert_eq!(parse_pressure(b""), None);
    }
}
