use super::Button;

const PREFIX: &str = "BREWCTL/1 ";
const MAX_LINE_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlCommand {
    Tap(Button),
    Status,
    Screen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlParseError {
    InvalidEncoding,
    LineTooLong,
    UnknownCommand,
}

impl ControlParseError {
    pub const fn name(self) -> &'static str {
        match self {
            Self::InvalidEncoding => "invalid-encoding",
            Self::LineTooLong => "line-too-long",
            Self::UnknownCommand => "unknown-command",
        }
    }
}

pub struct ControlLineBuffer {
    bytes: [u8; MAX_LINE_BYTES],
    length: usize,
    overflowed: bool,
}

impl ControlLineBuffer {
    pub const fn new() -> Self {
        Self {
            bytes: [0; MAX_LINE_BYTES],
            length: 0,
            overflowed: false,
        }
    }

    pub fn push(&mut self, byte: u8) -> Option<Result<ControlCommand, ControlParseError>> {
        if byte == b'\n' {
            if self.overflowed {
                self.reset();
                return Some(Err(ControlParseError::LineTooLong));
            }
            let length = self.length;
            self.length = 0;
            if length == 0 {
                return None;
            }
            let line = if self.bytes[length - 1] == b'\r' {
                &self.bytes[..length - 1]
            } else {
                &self.bytes[..length]
            };
            return Some(parse_control_command(line));
        }

        if self.overflowed {
            return None;
        }
        if self.length == self.bytes.len() {
            self.overflowed = true;
            return None;
        }
        self.bytes[self.length] = byte;
        self.length += 1;
        None
    }

    fn reset(&mut self) {
        self.length = 0;
        self.overflowed = false;
    }
}

impl Default for ControlLineBuffer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn parse_control_command(line: &[u8]) -> Result<ControlCommand, ControlParseError> {
    let line = core::str::from_utf8(line).map_err(|_| ControlParseError::InvalidEncoding)?;
    let command = line
        .strip_prefix(PREFIX)
        .ok_or(ControlParseError::UnknownCommand)?;

    match command {
        "status" => Ok(ControlCommand::Status),
        "screen" => Ok(ControlCommand::Screen),
        _ => command
            .strip_prefix("tap ")
            .and_then(Button::from_name)
            .map(ControlCommand::Tap)
            .ok_or(ControlParseError::UnknownCommand),
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{ControlCommand, ControlLineBuffer, ControlParseError, parse_control_command};
    use crate::input::Button;

    #[test]
    fn parses_every_button_tap() {
        let cases = [
            ("back", Button::Back),
            ("confirm", Button::Confirm),
            ("left", Button::Left),
            ("right", Button::Right),
            ("up", Button::Up),
            ("down", Button::Down),
            ("power", Button::Power),
        ];

        for (name, button) in cases {
            let mut line = std::string::String::from("BREWCTL/1 tap ");
            line.push_str(name);
            assert_eq!(
                parse_control_command(line.as_bytes()),
                Ok(ControlCommand::Tap(button))
            );
        }
    }

    #[test]
    fn parses_status_and_screen() {
        assert_eq!(
            parse_control_command(b"BREWCTL/1 status"),
            Ok(ControlCommand::Status)
        );
        assert_eq!(
            parse_control_command(b"BREWCTL/1 screen"),
            Ok(ControlCommand::Screen)
        );
    }

    #[test]
    fn rejects_unframed_and_unknown_commands() {
        assert_eq!(
            parse_control_command(b"tap right"),
            Err(ControlParseError::UnknownCommand)
        );
        assert_eq!(
            parse_control_command(b"BREWCTL/1 reset"),
            Err(ControlParseError::UnknownCommand)
        );
    }

    #[test]
    fn buffers_crlf_and_recovers_after_overflow() {
        let mut buffer = ControlLineBuffer::new();
        let mut result = None;
        for byte in b"BREWCTL/1 tap right\r\n" {
            result = buffer.push(*byte).or(result);
        }
        assert_eq!(result, Some(Ok(ControlCommand::Tap(Button::Right))));

        for byte in [b'x'; 40] {
            assert_eq!(buffer.push(byte), None);
        }
        assert_eq!(
            buffer.push(b'\n'),
            Some(Err(ControlParseError::LineTooLong))
        );

        let mut result = None;
        for byte in b"BREWCTL/1 status\n" {
            result = buffer.push(*byte).or(result);
        }
        assert_eq!(result, Some(Ok(ControlCommand::Status)));
    }
}
