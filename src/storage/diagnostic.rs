use super::Sector;

pub const MAX_READ_SECTORS: u32 = 8;
pub const MAX_READ_BYTES: usize = MAX_READ_SECTORS as usize * Sector::LEN;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectorRange {
    start: u32,
    count: u32,
}

impl SectorRange {
    pub fn new(start: u32, count: u32) -> Option<Self> {
        if count == 0 || count > MAX_READ_SECTORS {
            return None;
        }
        start.checked_add(count - 1)?;
        Some(Self { start, count })
    }

    pub const fn start(self) -> u32 {
        self.start
    }

    pub const fn count(self) -> u32 {
        self.count
    }

    pub const fn byte_length(self) -> usize {
        self.count as usize * Sector::LEN
    }

    pub const fn end(self) -> u64 {
        self.start as u64 + self.count as u64
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SdDiagnosticCommand {
    Info,
    Read(SectorRange),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidSdCommand;

impl SdDiagnosticCommand {
    pub fn parse(line: &[u8]) -> Result<Self, InvalidSdCommand> {
        let text = core::str::from_utf8(line).map_err(|_| InvalidSdCommand)?;
        let text = text.strip_prefix("BREWCTL/1 ").ok_or(InvalidSdCommand)?;
        let mut words = text.split_ascii_whitespace();
        let command = match words.next() {
            Some("sd-info") => Self::Info,
            Some("sd-read") => {
                let start = words.next().ok_or(InvalidSdCommand)?;
                let count = words.next().ok_or(InvalidSdCommand)?;
                if !start.bytes().all(|byte| byte.is_ascii_digit())
                    || !count.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err(InvalidSdCommand);
                }
                let range = SectorRange::new(
                    start.parse().map_err(|_| InvalidSdCommand)?,
                    count.parse().map_err(|_| InvalidSdCommand)?,
                )
                .ok_or(InvalidSdCommand)?;
                Self::Read(range)
            }
            _ => return Err(InvalidSdCommand),
        };
        if words.next().is_some() {
            return Err(InvalidSdCommand);
        }
        Ok(command)
    }
}

pub struct SdDiagnosticLines {
    bytes: [u8; 64],
    length: usize,
    overflowed: bool,
}

impl Default for SdDiagnosticLines {
    fn default() -> Self {
        Self {
            bytes: [0; 64],
            length: 0,
            overflowed: false,
        }
    }
}

impl SdDiagnosticLines {
    pub fn push(&mut self, byte: u8) -> Option<Result<SdDiagnosticCommand, InvalidSdCommand>> {
        if byte == b'\n' {
            let result = if self.overflowed {
                Err(InvalidSdCommand)
            } else {
                SdDiagnosticCommand::parse(&self.bytes[..self.length])
            };
            self.length = 0;
            self.overflowed = false;
            return Some(result);
        }
        if self.length == self.bytes.len() {
            self.overflowed = true;
        } else if !self.overflowed {
            self.bytes[self.length] = byte;
            self.length += 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{MAX_READ_BYTES, SdDiagnosticCommand, SdDiagnosticLines, SectorRange};

    #[test]
    fn read_ranges_are_bounded_and_do_not_wrap() {
        assert!(SectorRange::new(0, 0).is_none());
        assert!(SectorRange::new(0, 9).is_none());
        assert!(SectorRange::new(u32::MAX, 2).is_none());
        let last = SectorRange::new(u32::MAX, 1).unwrap();
        assert_eq!(last.end(), u64::from(u32::MAX) + 1);
        let chunk = SectorRange::new(5, 8).unwrap();
        assert_eq!(chunk.start(), 5);
        assert_eq!(chunk.count(), 8);
        assert_eq!(chunk.byte_length(), MAX_READ_BYTES);
        assert_eq!(chunk.end(), 13);
    }

    #[test]
    fn diagnostic_commands_cannot_request_writes() {
        for command in [
            "sd-read 0 0",
            "sd-read 0 9",
            "sd-read 4294967295 2",
            "sd-read -1 1",
            "sd-read +1 1",
            "sd-read 0 1 extra",
            "sd-info extra",
            "sd-write 0 1",
            "upload NICE.JPG 512 0",
            "erase",
            "format",
            "tap confirm",
        ] {
            let line = std::format!("BREWCTL/1 {command}");
            assert!(
                SdDiagnosticCommand::parse(line.as_bytes()).is_err(),
                "{command}"
            );
        }
        assert!(SdDiagnosticCommand::parse(b"sd-info").is_err());
        assert!(SdDiagnosticCommand::parse(b"BREWCTL/1 \xff").is_err());
        assert_eq!(
            SdDiagnosticCommand::parse(b"BREWCTL/1 sd-info"),
            Ok(SdDiagnosticCommand::Info)
        );
        assert_eq!(
            SdDiagnosticCommand::parse(b"BREWCTL/1 sd-read 123 8"),
            Ok(SdDiagnosticCommand::Read(SectorRange::new(123, 8).unwrap()))
        );
    }

    #[test]
    fn overflowing_lines_are_discarded_until_newline() {
        let mut lines = SdDiagnosticLines::default();
        for _ in 0..80 {
            assert!(lines.push(b'x').is_none());
        }
        for byte in b"BREWCTL/1 sd-info" {
            assert!(lines.push(*byte).is_none());
        }
        assert!(lines.push(b'\n').unwrap().is_err());
        for byte in b"BREWCTL/1 sd-info\r" {
            assert!(lines.push(*byte).is_none());
        }
        assert_eq!(lines.push(b'\n'), Some(Ok(SdDiagnosticCommand::Info)));
    }
}
