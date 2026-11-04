use monitor_core::Percent;
use monitor_core::util::parse::{fields, lines, parse_f64};

use crate::pressure::PressureAverages;

/// The `some` and `full` lines of a `/proc/pressure/*` file.
pub(crate) fn parse_pressure(
    buffer: &[u8],
) -> (Option<PressureAverages>, Option<PressureAverages>) {
    let mut some = None;
    let mut full = None;
    for line in lines(buffer) {
        let mut columns = fields(line);
        let slot = match columns.next() {
            Some(b"some") => &mut some,
            Some(b"full") => &mut full,
            _ => continue,
        };
        let mut averages = PressureAverages::default();
        for column in columns {
            let Some(equals) = memchr::memchr(b'=', column) else {
                continue;
            };
            let value = Percent::new(parse_f64(&column[equals + 1..]).unwrap_or(0.0) as f32);
            match &column[..equals] {
                b"avg10" => averages.avg10 = value,
                b"avg60" => averages.avg60 = value,
                b"avg300" => averages.avg300 = value,
                _ => {}
            }
        }
        *slot = Some(averages);
    }
    (some, full)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMORY_PRESSURE: &[u8] = b"some avg10=1.50 avg60=0.00 avg300=0.06 total=2002847\n\
        full avg10=0.25 avg60=0.00 avg300=0.06 total=1968340\n";

    #[test]
    fn parses_some_and_full() {
        let (some, full) = parse_pressure(MEMORY_PRESSURE);
        assert_eq!(some.map(|line| line.avg10.get()), Some(1.5));
        assert_eq!(some.map(|line| line.avg300.get()), Some(0.06));
        assert_eq!(full.map(|line| line.avg10.get()), Some(0.25));
        assert_eq!(parse_pressure(b""), (None, None));
    }
}
