#[cfg(not(unix))]
compile_error!("device-control requires a Unix host");

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};

use brewthink::input::Button;
use image::ImageEncoder;

const CONTROL_PREFIX: &[u8] = b"BREWCTL/1 ";
const FRAME_WIDTH: u32 = 480;
const FRAME_HEIGHT: u32 = 800;
const FRAME_BYTES: usize = FRAME_WIDTH as usize * FRAME_HEIGHT as usize / 8;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(90);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Tap(Button),
    Status,
    Screen(PathBuf),
    Monitor,
    Help,
}

#[derive(Debug, Eq, PartialEq)]
struct Arguments {
    port: Option<PathBuf>,
    timeout: Duration,
    command: Command,
}

#[derive(Debug, Eq, PartialEq)]
struct ScreenHeader {
    width: u32,
    height: u32,
    length: usize,
    crc32: u32,
}

#[derive(Default)]
struct ControlStream {
    buffer: Vec<u8>,
}

impl ControlStream {
    fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    fn take_line(&mut self) -> Option<Vec<u8>> {
        let marker = find_bytes(&self.buffer, CONTROL_PREFIX);
        let Some(marker) = marker else {
            let retained = self.buffer.len().min(CONTROL_PREFIX.len() - 1);
            if retained > 0 {
                self.buffer.drain(..self.buffer.len() - retained);
            }
            return None;
        };
        let newline = self.buffer[marker..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| marker + offset)?;
        let mut line = self.buffer[marker..newline].to_vec();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        self.buffer.drain(..=newline);
        Some(line)
    }

    fn take_bytes(&mut self, length: usize) -> Option<Vec<u8>> {
        if self.buffer.len() < length {
            return None;
        }
        Some(self.buffer.drain(..length).collect())
    }

    fn clear(&mut self) {
        self.buffer.clear();
    }
}

struct Connection {
    file: File,
    stream: ControlStream,
}

impl Connection {
    fn open(path: &Path, writable: bool) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(writable)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
        Ok(Self {
            file: options.open(path)?,
            stream: ControlStream::default(),
        })
    }

    fn drain(&mut self) -> io::Result<()> {
        self.stream.clear();
        let mut bytes = [0; 4096];
        loop {
            match self.file.read(&mut bytes) {
                Ok(0) => return Ok(()),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }

    fn write_all(&mut self, bytes: &[u8], deadline: Instant) -> io::Result<()> {
        let mut position = 0;
        while position < bytes.len() {
            match self.file.write(&bytes[position..]) {
                Ok(0) => return Err(disconnected()),
                Ok(written) => position += written,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    wait_until_ready(deadline, "device command write timed out")?;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn read_line(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        loop {
            if let Some(line) = self.stream.take_line() {
                return Ok(line);
            }
            self.read_more(deadline)?;
        }
    }

    fn read_payload(&mut self, length: usize, deadline: Instant) -> io::Result<Vec<u8>> {
        loop {
            if let Some(bytes) = self.stream.take_bytes(length) {
                return Ok(bytes);
            }
            self.read_more(deadline)?;
        }
    }

    fn read_more(&mut self, deadline: Instant) -> io::Result<()> {
        let mut bytes = [0; 4096];
        loop {
            match self.file.read(&mut bytes) {
                Ok(0) => return Err(disconnected()),
                Ok(length) => {
                    self.stream.push(&bytes[..length]);
                    return Ok(());
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    wait_until_ready(deadline, "device response timed out")?;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> io::Result<()> {
    let arguments = parse_arguments(env::args().skip(1), env::var_os("ESPFLASH_PORT"))?;
    if arguments.command == Command::Help {
        print_usage();
        return Ok(());
    }
    if arguments.command == Command::Monitor {
        return monitor(arguments.port.as_deref());
    }

    let port = find_port(arguments.port.as_deref())?;
    let mut connection = Connection::open(&port, true)?;
    match arguments.command {
        Command::Tap(button) => run_text_command(
            &mut connection,
            &format!("tap {}", button.name()),
            arguments.timeout,
        ),
        Command::Status => run_text_command(&mut connection, "status", arguments.timeout),
        Command::Screen(output) => capture_screen(&mut connection, &output, arguments.timeout),
        Command::Monitor | Command::Help => unreachable!(),
    }
}

fn parse_arguments(
    arguments: impl IntoIterator<Item = String>,
    environment_port: Option<std::ffi::OsString>,
) -> io::Result<Arguments> {
    let mut arguments = arguments.into_iter().peekable();
    let mut port = environment_port.map(PathBuf::from);
    let mut timeout = DEFAULT_TIMEOUT;

    loop {
        match arguments.peek().map(String::as_str) {
            Some("--port") => {
                arguments.next();
                port = Some(PathBuf::from(required_argument(&mut arguments, "--port")?));
            }
            Some("--timeout") => {
                arguments.next();
                let value = required_argument(&mut arguments, "--timeout")?;
                let seconds = value
                    .parse::<f64>()
                    .map_err(|_| invalid_input("--timeout must be a number"))?;
                if !seconds.is_finite() || seconds <= 0.0 {
                    return Err(invalid_input("--timeout must be greater than zero"));
                }
                timeout = Duration::from_secs_f64(seconds);
            }
            _ => break,
        }
    }

    let command = match arguments.next().as_deref() {
        Some("tap") => {
            let name = required_argument(&mut arguments, "tap")?;
            let button =
                Button::from_name(&name).ok_or_else(|| invalid_input("unknown button name"))?;
            Command::Tap(button)
        }
        Some("status") => Command::Status,
        Some("screen") => {
            Command::Screen(PathBuf::from(required_argument(&mut arguments, "screen")?))
        }
        Some("monitor") => Command::Monitor,
        Some("--help" | "-h") => Command::Help,
        Some(_) => return Err(invalid_input("unknown command")),
        None => return Err(invalid_input("missing command")),
    };

    if arguments.next().is_some() {
        return Err(invalid_input("unexpected argument"));
    }

    Ok(Arguments {
        port,
        timeout,
        command,
    })
}

fn required_argument(
    arguments: &mut impl Iterator<Item = String>,
    option: &str,
) -> io::Result<String> {
    arguments
        .next()
        .ok_or_else(|| invalid_input(format!("missing value for {option}")))
}

fn print_usage() {
    println!(
        "Usage: device-control [--port PATH] [--timeout SECONDS] <COMMAND>\n\n\
         Commands:\n  tap <back|confirm|left|right|up|down|power>\n  status\n  screen <OUTPUT.png>\n  monitor"
    );
}

fn find_port(requested: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(requested) = requested
        && requested.exists()
    {
        return Ok(requested.to_path_buf());
    }

    let mut ports = fs::read_dir("/dev")?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("cu.usbmodem"))
                .then(|| entry.path())
        })
        .collect::<Vec<_>>();
    ports.sort();

    match ports.as_slice() {
        [port] => Ok(port.clone()),
        _ => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "expected exactly one /dev/cu.usbmodem device, found {}",
                ports.len()
            ),
        )),
    }
}

fn run_text_command(
    connection: &mut Connection,
    command: &str,
    timeout: Duration,
) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    connection.drain()?;
    connection.write_all(format!("BREWCTL/1 {command}\n").as_bytes(), deadline)?;
    let command_name = command.split_whitespace().next().unwrap_or(command);
    let terminal = format!("BREWCTL/1 DONE command={command_name} ");

    loop {
        let line = connection.read_line(deadline)?;
        print_control_line(&line);
        if line.starts_with(terminal.as_bytes()) {
            if !line.ends_with(b"status=ok") {
                return Err(invalid_data(String::from_utf8_lossy(&line)));
            }
            return Ok(());
        }
    }
}

fn capture_screen(connection: &mut Connection, output: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    connection.drain()?;
    connection.write_all(b"BREWCTL/1 screen\n", deadline)?;

    let header = loop {
        let line = connection.read_line(deadline)?;
        print_control_line(&line);
        if line.starts_with(b"BREWCTL/1 SCREEN ") {
            break parse_screen_header(&line)?;
        }
    };

    if (header.width, header.height, header.length) != (FRAME_WIDTH, FRAME_HEIGHT, FRAME_BYTES) {
        return Err(invalid_data(format!(
            "unexpected frame shape {}x{}, {} bytes",
            header.width, header.height, header.length
        )));
    }
    let frame = connection.read_payload(header.length, deadline)?;
    let actual_crc = crc32fast::hash(&frame);
    if actual_crc != header.crc32 {
        return Err(invalid_data(format!(
            "frame checksum mismatch: expected {:08x}, got {actual_crc:08x}",
            header.crc32
        )));
    }

    loop {
        let line = connection.read_line(deadline)?;
        print_control_line(&line);
        if line.starts_with(b"BREWCTL/1 DONE command=screen ") {
            if !line.ends_with(b"status=ok") {
                return Err(invalid_data(String::from_utf8_lossy(&line)));
            }
            break;
        }
    }

    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        output,
        encode_frame_png(&frame, header.width, header.height)?,
    )?;
    println!(
        "host: wrote {} frame_crc32={actual_crc:08x}",
        output.display()
    );
    Ok(())
}

fn parse_screen_header(line: &[u8]) -> io::Result<ScreenHeader> {
    let line = std::str::from_utf8(line).map_err(|_| invalid_data("screen header is not UTF-8"))?;
    let fields = line
        .strip_prefix("BREWCTL/1 SCREEN ")
        .ok_or_else(|| invalid_data("not a screen header"))?;
    let mut width = None;
    let mut height = None;
    let mut length = None;
    let mut crc32 = None;

    for field in fields.split_whitespace() {
        let Some((name, value)) = field.split_once('=') else {
            return Err(invalid_data("malformed screen header field"));
        };
        match name {
            "width" => width = Some(parse_decimal(value, "width")?),
            "height" => height = Some(parse_decimal(value, "height")?),
            "bytes" => length = Some(parse_decimal(value, "bytes")?),
            "crc32" => {
                crc32 = Some(
                    u32::from_str_radix(value, 16)
                        .map_err(|_| invalid_data("invalid screen crc32"))?,
                )
            }
            _ => {}
        }
    }

    Ok(ScreenHeader {
        width: width.ok_or_else(|| invalid_data("screen width is missing"))?,
        height: height.ok_or_else(|| invalid_data("screen height is missing"))?,
        length: length.ok_or_else(|| invalid_data("screen byte length is missing"))?,
        crc32: crc32.ok_or_else(|| invalid_data("screen crc32 is missing"))?,
    })
}

fn parse_decimal<T>(value: &str, name: &str) -> io::Result<T>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| invalid_data(format!("invalid screen {name}")))
}

fn encode_frame_png(frame: &[u8], width: u32, height: u32) -> io::Result<Vec<u8>> {
    let row_bytes = width as usize / 8;
    if !width.is_multiple_of(8) || frame.len() != row_bytes * height as usize {
        return Err(invalid_data("frame dimensions do not match packed bytes"));
    }

    let mut pixels = Vec::with_capacity(width as usize * height as usize);
    for byte in frame {
        for bit in (0..8).rev() {
            pixels.push(if byte & (1 << bit) == 0 { 0 } else { 255 });
        }
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&pixels, width, height, image::ExtendedColorType::L8)
        .map_err(io::Error::other)?;
    Ok(png)
}

fn monitor(requested_port: Option<&Path>) -> io::Result<()> {
    loop {
        let port = loop {
            match find_port(requested_port) {
                Ok(port) => break port,
                Err(_) => thread::sleep(Duration::from_millis(250)),
            }
        };
        let mut connection = match Connection::open(&port, false) {
            Ok(connection) => connection,
            Err(_) => {
                thread::sleep(Duration::from_millis(250));
                continue;
            }
        };
        println!("host: connected port={}", port.display());

        loop {
            match connection.read_line(Instant::now() + Duration::from_millis(500)) {
                Ok(line) => print_control_line(&line),
                Err(error)
                    if error.kind() == io::ErrorKind::TimedOut && port.as_path().exists() => {}
                Err(_) => {
                    println!("host: disconnected port={}", port.display());
                    break;
                }
            }
        }
    }
}

fn print_control_line(line: &[u8]) {
    println!("{}", String::from_utf8_lossy(line));
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn wait_until_ready(deadline: Instant, message: &str) -> io::Result<()> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, message))?;
    thread::sleep(POLL_INTERVAL.min(remaining));
    Ok(())
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn disconnected() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "device disconnected")
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, io::Cursor, path::PathBuf, time::Duration};

    use image::GenericImageView;

    use super::{
        Arguments, Command, ControlStream, DEFAULT_TIMEOUT, FRAME_BYTES, FRAME_HEIGHT, FRAME_WIDTH,
        ScreenHeader, encode_frame_png, parse_arguments, parse_screen_header,
    };
    use brewthink::input::Button;

    #[test]
    fn parses_cli_commands() {
        assert_eq!(
            parse_arguments(["tap".into(), "right".into()], None).unwrap(),
            Arguments {
                port: None,
                timeout: DEFAULT_TIMEOUT,
                command: Command::Tap(Button::Right),
            }
        );
        assert_eq!(
            parse_arguments(
                ["--timeout".into(), "12.5".into(), "status".into(),],
                Some(OsString::from("/dev/cu.usbmodem-test")),
            )
            .unwrap(),
            Arguments {
                port: Some(PathBuf::from("/dev/cu.usbmodem-test")),
                timeout: Duration::from_secs_f64(12.5),
                command: Command::Status,
            }
        );
    }

    #[test]
    fn extracts_framed_lines_from_mixed_serial_bytes() {
        let mut stream = ControlStream::default();
        stream.push(b"\x00\xa5defmt\nnoiseBREWCTL/1 EVENT source=usb input=right\r\nrest");

        assert_eq!(
            stream.take_line().unwrap(),
            b"BREWCTL/1 EVENT source=usb input=right"
        );
        assert_eq!(stream.take_line(), None);
    }

    #[test]
    fn preserves_screen_payload_after_header() {
        let mut stream = ControlStream::default();
        stream.push(
            b"noiseBREWCTL/1 SCREEN width=480 height=800 bytes=4 crc32=00000000\n\
              \x00\n\xff\x80",
        );

        assert_eq!(
            stream.take_line().unwrap(),
            b"BREWCTL/1 SCREEN width=480 height=800 bytes=4 crc32=00000000"
        );
        assert_eq!(stream.take_bytes(4).unwrap(), b"\x00\n\xff\x80");
    }

    #[test]
    fn parses_screen_header_fields() {
        assert_eq!(
            parse_screen_header(
                b"BREWCTL/1 SCREEN width=480 height=800 bytes=48000 crc32=e4e73d7c"
            )
            .unwrap(),
            ScreenHeader {
                width: FRAME_WIDTH,
                height: FRAME_HEIGHT,
                length: FRAME_BYTES,
                crc32: 0xe4e7_3d7c,
            }
        );
    }

    #[test]
    fn encodes_packed_pixels_as_png() {
        let frame = vec![0xaa; FRAME_BYTES];
        let encoded = encode_frame_png(&frame, FRAME_WIDTH, FRAME_HEIGHT).unwrap();
        let decoded = image::ImageReader::new(Cursor::new(encoded))
            .with_guessed_format()
            .unwrap()
            .decode()
            .unwrap();

        assert_eq!(decoded.dimensions(), (FRAME_WIDTH, FRAME_HEIGHT));
        assert_eq!(
            decoded.to_luma8().as_raw()[..8],
            [255, 0, 255, 0, 255, 0, 255, 0]
        );
    }
}
