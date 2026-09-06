use super::Button;
use crate::transfer::{ImageName, UploadRequest};

const PREFIX: &str = "BREWCTL/1 ";
const MAX_LINE_BYTES: usize = 96;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlCommand {
    Tap(Button),
    Status,
    Screen,
    Upload(UploadRequest),
    AbortUpload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlParseError {
    InvalidEncoding,
    LineTooLong,
    InvalidUpload,
    UnknownCommand,
}

impl ControlParseError {
    pub const fn name(self) -> &'static str {
        match self {
            Self::InvalidEncoding => "invalid-encoding",
            Self::LineTooLong => "line-too-long",
            Self::InvalidUpload => "invalid-upload",
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
        "upload-abort" => Ok(ControlCommand::AbortUpload),
        _ => {
            if let Some(arguments) = command.strip_prefix("upload ") {
                return parse_upload(arguments).map(ControlCommand::Upload);
            }
            command
                .strip_prefix("tap ")
                .and_then(Button::from_name)
                .map(ControlCommand::Tap)
                .ok_or(ControlParseError::UnknownCommand)
        }
    }
}

fn parse_upload(arguments: &str) -> Result<UploadRequest, ControlParseError> {
    let mut fields = arguments.split(' ');
    if fields.next() != Some("image") {
        return Err(ControlParseError::InvalidUpload);
    }
    let name = fields
        .next()
        .and_then(|value| ImageName::parse(value).ok())
        .ok_or(ControlParseError::InvalidUpload)?;
    let length = fields
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or(ControlParseError::InvalidUpload)?;
    let crc32 = fields
        .next()
        .and_then(|value| u32::from_str_radix(value, 16).ok())
        .ok_or(ControlParseError::InvalidUpload)?;
    if fields.next().is_some() {
        return Err(ControlParseError::InvalidUpload);
    }
    Ok(UploadRequest::image(name, length, crc32))
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{ControlCommand, ControlLineBuffer, ControlParseError, parse_control_command};
    use crate::{
        input::Button,
        transfer::{ImageName, UploadRequest},
    };

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
    fn parses_status_screen_and_upload() {
        assert_eq!(
            parse_control_command(b"BREWCTL/1 status"),
            Ok(ControlCommand::Status)
        );
        assert_eq!(
            parse_control_command(b"BREWCTL/1 screen"),
            Ok(ControlCommand::Screen)
        );
        assert_eq!(
            parse_control_command(b"BREWCTL/1 upload image NICE.PNG 123 89abcdef"),
            Ok(ControlCommand::Upload(UploadRequest::image(
                ImageName::parse("NICE.PNG").unwrap(),
                123,
                0x89AB_CDEF,
            )))
        );
        assert_eq!(
            parse_control_command(b"BREWCTL/1 upload-abort"),
            Ok(ControlCommand::AbortUpload)
        );
    }

    #[test]
    fn rejects_unframed_unknown_and_malformed_commands() {
        assert_eq!(
            parse_control_command(b"tap right"),
            Err(ControlParseError::UnknownCommand)
        );
        assert_eq!(
            parse_control_command(b"BREWCTL/1 reset"),
            Err(ControlParseError::UnknownCommand)
        );
        assert_eq!(
            parse_control_command(b"BREWCTL/1 upload image bad.gif 12 nope"),
            Err(ControlParseError::InvalidUpload)
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

        for byte in [b'x'; 100] {
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
