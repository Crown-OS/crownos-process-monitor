use std::fmt::Display;
use std::io::{self, Write};

const SEPARATOR: u8 = b',';
const QUOTE: u8 = b'"';
const RECORD_END: &[u8] = b"\n";

/// Minimal RFC 4180 writer: text fields containing a comma, quote or line
/// break are quoted, with inner quotes doubled.
pub struct CsvWriter<W: Write> {
    output: W,
    at_record_start: bool,
}

impl<W: Write> CsvWriter<W> {
    pub fn new(output: W) -> Self {
        Self {
            output,
            at_record_start: true,
        }
    }

    fn separate(&mut self) -> io::Result<()> {
        if !std::mem::replace(&mut self.at_record_start, false) {
            self.output.write_all(&[SEPARATOR])?;
        }
        Ok(())
    }

    pub fn text(&mut self, value: &str) -> io::Result<()> {
        self.separate()?;
        if !value.contains([',', '"', '\n', '\r']) {
            return self.output.write_all(value.as_bytes());
        }
        self.output.write_all(&[QUOTE])?;
        for (index, part) in value.split('"').enumerate() {
            if index > 0 {
                self.output.write_all(&[QUOTE, QUOTE])?;
            }
            self.output.write_all(part.as_bytes())?;
        }
        self.output.write_all(&[QUOTE])
    }

    /// Writes a value whose `Display` never needs quoting (numbers).
    pub fn number(&mut self, value: impl Display) -> io::Result<()> {
        self.separate()?;
        write!(self.output, "{value}")
    }

    pub fn empty(&mut self) -> io::Result<()> {
        self.separate()
    }

    pub fn end_record(&mut self) -> io::Result<()> {
        self.at_record_start = true;
        self.output.write_all(RECORD_END)
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.output.flush()?;
        Ok(self.output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_only_when_needed() -> io::Result<()> {
        let mut writer = CsvWriter::new(Vec::new());
        writer.text("plain")?;
        writer.text("a,b")?;
        writer.text("say \"hi\"")?;
        writer.text("two\nlines")?;
        writer.number(1.5)?;
        writer.empty()?;
        writer.end_record()?;
        writer.number(7)?;
        writer.end_record()?;
        let written = writer.finish()?;
        assert_eq!(
            String::from_utf8_lossy(&written),
            "plain,\"a,b\",\"say \"\"hi\"\"\",\"two\nlines\",1.5,\n7\n"
        );
        Ok(())
    }
}
