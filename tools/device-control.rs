#[cfg(not(unix))]
compile_error!("device-control requires a Unix host");

mod sd_export;

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};

use brewthink::{
    image::{PackedBitmap, PackedImage, PixelDepth, RenderOptions, ScaleMode, Size},
    image_decoder::{
        ImageFormat, JpegDecodeWorkspace, PngDecodeWorkspace, decode_jpeg, decode_png,
    },
    input::Button,
    transfer::{ImageName, MAX_IMAGE_BYTES},
};
use image::{ExtendedColorType, ImageEncoder, codecs::jpeg::JpegEncoder, imageops::FilterType};

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
    PutImage(PathBuf),
    SdInfo,
    SdRead {
        start: u32,
        count: u32,
        output: PathBuf,
    },
    Monitor,
    Help,
}

struct PreparedImage {
    name: ImageName,
    bytes: Vec<u8>,
    transcoded: bool,
}

#[derive(Debug, Eq, PartialEq)]
struct Arguments {
    port: Option<PathBuf>,
    timeout: Duration,
    command: Command,
}

#[derive(Debug, Eq, PartialEq)]
struct ScreenHeader {
    depth: PixelDepth,
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
    original_termios: Option<libc::termios>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        if let Some(original) = &self.original_termios {
            // SAFETY: the connection still owns its open descriptor and the saved attributes.
            unsafe {
                libc::tcsetattr(self.file.as_raw_fd(), libc::TCSANOW, original);
            }
        }
    }
}

impl Connection {
    fn open(path: &Path, writable: bool) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(writable)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
        let file = options.open(path)?;
        let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
        // SAFETY: tcgetattr initializes the output on success; the descriptor is owned here.
        if unsafe { libc::tcgetattr(file.as_raw_fd(), original.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: tcgetattr succeeded above.
        let original = unsafe { original.assume_init() };
        let mut raw = original;
        // SAFETY: cfmakeraw and tcsetattr receive valid termios storage and the live descriptor.
        unsafe {
            libc::cfmakeraw(&mut raw);
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(file.as_raw_fd(), libc::TCSANOW, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(Self {
            file,
            stream: ControlStream::default(),
            original_termios: Some(original),
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

    let prepared_image = match &arguments.command {
        Command::PutImage(input) => {
            let prepared = prepare_image(input)?;
            println!(
                "host: prepared {} as {} bytes={}{}",
                input.display(),
                prepared.name.as_str(),
                prepared.bytes.len(),
                if prepared.transcoded {
                    " transcoded=yes"
                } else {
                    ""
                }
            );
            Some(prepared)
        }
        _ => None,
    };
    let port = find_port(arguments.port.as_deref())?;
    let mut connection = Connection::open(&port, true)?;
    match arguments.command {
        Command::Tap(button) => run_text_command(
            &mut connection,
            &format!("tap {}", button.name()),
            arguments.timeout,
        ),
        Command::Status => run_text_command(&mut connection, "status", arguments.timeout),
        Command::SdInfo => sd_export::info(&mut connection, arguments.timeout).map(|_| ()),
        Command::SdRead {
            start,
            count,
            output,
        } => sd_export::export(&mut connection, start, count, &output, arguments.timeout),
        Command::Screen(output) => capture_screen(&mut connection, &output, arguments.timeout),
        Command::PutImage(input) => upload_image(
            &mut connection,
            &input,
            prepared_image.expect("put-image preparation ran before opening the port"),
            arguments.timeout,
        ),
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
        Some("put-image") => Command::PutImage(PathBuf::from(required_argument(
            &mut arguments,
            "put-image",
        )?)),
        Some("sd-info") => Command::SdInfo,
        Some("sd-read") => {
            let start = required_argument(&mut arguments, "start sector")?
                .parse()
                .map_err(|_| invalid_input("start sector must be a u32 decimal integer"))?;
            let count = required_argument(&mut arguments, "sector count")?
                .parse()
                .map_err(|_| invalid_input("sector count must be a u32 decimal integer"))?;
            if count == 0 || u64::from(start) + u64::from(count) > u64::from(u32::MAX) + 1 {
                return Err(invalid_input(
                    "SD range is empty or overflows sector addressing",
                ));
            }
            let output = PathBuf::from(required_argument(&mut arguments, "output file")?);
            Command::SdRead {
                start,
                count,
                output,
            }
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
         Commands:\n  tap <back|confirm|left|right|up|down|power>\n  status\n  screen <OUTPUT.png>\n  put-image <INPUT.jpg|INPUT.png>\n  sd-info\n  sd-read <START_SECTOR> <SECTOR_COUNT> <OUTPUT.bin>\n  monitor"
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

    if (header.width, header.height, header.length)
        != (FRAME_WIDTH, FRAME_HEIGHT, FRAME_BYTES * header.depth.bits())
    {
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
        encode_frame_png(&frame, header.width, header.height, header.depth)?,
    )?;
    println!(
        "host: wrote {} frame_crc32={actual_crc:08x}",
        output.display()
    );
    Ok(())
}

fn upload_image(
    connection: &mut Connection,
    input: &Path,
    prepared: PreparedImage,
    timeout: Duration,
) -> io::Result<()> {
    let bytes = &prepared.bytes;
    let deadline = Instant::now() + timeout;
    let crc32 = crc32fast::hash(bytes);
    connection.drain()?;
    connection.write_all(
        format!(
            "BREWCTL/1 upload image {} {} {crc32:08x}\n",
            prepared.name.as_str(),
            bytes.len()
        )
        .as_bytes(),
        deadline,
    )?;
    loop {
        let line = connection.read_line(deadline)?;
        print_control_line(&line);
        if line.starts_with(b"BREWCTL/1 READY command=upload ") {
            break;
        }
        if line.starts_with(b"BREWCTL/1 DONE command=upload status=error") {
            return Err(invalid_data("device rejected the upload"));
        }
    }

    let mut sent = 0usize;
    for chunk in bytes.chunks(4 * 1024) {
        connection.write_all(chunk, deadline)?;
        sent += chunk.len();
        loop {
            let line = connection.read_line(deadline)?;
            print_control_line(&line);
            if line.starts_with(b"BREWCTL/1 ACK command=upload ") {
                let received = parse_control_field::<usize>(&line, "received")?;
                if received != sent {
                    return Err(invalid_data(format!(
                        "upload acknowledgement expected {sent} bytes, got {received}"
                    )));
                }
                break;
            }
            if line.starts_with(b"BREWCTL/1 DONE command=upload status=error") {
                return Err(invalid_data("device upload failed"));
            }
        }
    }

    loop {
        let line = connection.read_line(deadline)?;
        print_control_line(&line);
        if line.starts_with(b"BREWCTL/1 DONE command=upload ") {
            if !line.ends_with(b"status=ok") {
                return Err(invalid_data(String::from_utf8_lossy(&line)));
            }
            break;
        }
    }
    println!(
        "host: uploaded {} as {} bytes={} crc32={crc32:08x}",
        input.display(),
        prepared.name.as_str(),
        bytes.len()
    );
    Ok(())
}

fn prepare_image(input: &Path) -> io::Result<PreparedImage> {
    let source = fs::read(input)?;
    if source.is_empty() {
        return Err(invalid_input("image is empty"));
    }
    let source_format =
        ImageFormat::detect(&source).ok_or_else(|| invalid_input("image must be a JPEG or PNG"))?;
    let source_name = device_image_name(input, source_format)?;
    if source.len() <= MAX_IMAGE_BYTES && validate_device_image(source_format, &source).is_ok() {
        return Ok(PreparedImage {
            name: source_name,
            bytes: source,
            transcoded: false,
        });
    }

    transcode_image(input, &source)
}

fn transcode_image(input: &Path, source: &[u8]) -> io::Result<PreparedImage> {
    let decoded = image::load_from_memory(source)
        .map_err(|error| invalid_input(format!("image could not be decoded: {error}")))?;
    let resized = decoded
        .resize_to_fill(FRAME_WIDTH, FRAME_HEIGHT, FilterType::Lanczos3)
        .to_rgba8();
    let mut rgb = Vec::with_capacity(FRAME_WIDTH as usize * FRAME_HEIGHT as usize * 3);
    for pixel in resized.pixels() {
        let alpha = u32::from(pixel[3]);
        for channel in &pixel.0[..3] {
            rgb.push(((u32::from(*channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8);
        }
    }
    let name = device_image_name(input, ImageFormat::Jpeg)?;
    for quality in [85, 70, 55, 40, 30, 20, 10] {
        let mut encoded = Vec::new();
        JpegEncoder::new_with_quality(&mut encoded, quality)
            .encode(&rgb, FRAME_WIDTH, FRAME_HEIGHT, ExtendedColorType::Rgb8)
            .map_err(|error| invalid_data(format!("JPEG conversion failed: {error}")))?;
        if encoded.len() <= MAX_IMAGE_BYTES
            && validate_device_image(ImageFormat::Jpeg, &encoded).is_ok()
        {
            return Ok(PreparedImage {
                name,
                bytes: encoded,
                transcoded: true,
            });
        }
    }
    Err(invalid_input("image could not fit the device upload limit"))
}

fn device_image_name(input: &Path, format: ImageFormat) -> io::Result<ImageName> {
    let stem = input
        .file_stem()
        .ok_or_else(|| invalid_input("image filename has no stem"))?
        .to_string_lossy();
    let extension = match format {
        ImageFormat::Jpeg => "JPG",
        ImageFormat::Png => "PNG",
    };
    if let Ok(name) = ImageName::parse(&format!("{stem}.{extension}")) {
        return Ok(name);
    }

    let mut prefix = String::with_capacity(3);
    for character in stem
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
    {
        prefix.push(character.to_ascii_uppercase());
        if prefix.len() == 3 {
            break;
        }
    }
    if prefix.is_empty() {
        prefix.push_str("IMG");
    }
    let suffix = crc32fast::hash(stem.as_bytes()) as u16;
    ImageName::parse(&format!("{prefix}~{suffix:04X}.{extension}"))
        .map_err(|_| invalid_input("image filename could not be converted to FAT 8.3"))
}

fn validate_device_image(format: ImageFormat, encoded: &[u8]) -> io::Result<()> {
    let mut frame = vec![0xFF; FRAME_BYTES];
    let mut target = PackedImage::monochrome(
        Size::new(FRAME_WIDTH as usize, FRAME_HEIGHT as usize).unwrap(),
        &mut frame,
    )
    .expect("the host validation frame has the exact required length");
    let options = RenderOptions {
        scale: ScaleMode::Cover,
        ..RenderOptions::default()
    };
    let result = match format {
        ImageFormat::Jpeg => {
            let mut bytes = vec![0; core::mem::size_of::<JpegDecodeWorkspace>()];
            decode_jpeg(
                encoded,
                &mut target,
                options,
                JpegDecodeWorkspace::in_buffer(&mut bytes)
                    .expect("the JPEG validation workspace has the exact required length"),
            )
        }
        ImageFormat::Png => {
            let mut bytes = vec![0; core::mem::size_of::<PngDecodeWorkspace>()];
            decode_png(
                encoded,
                &mut target,
                options,
                PngDecodeWorkspace::in_buffer(&mut bytes)
                    .expect("the PNG validation workspace has the exact required length"),
            )
        }
    };
    result
        .map(|_| ())
        .map_err(|error| invalid_input(format!("image is not device-decodable: {error:?}")))
}

fn parse_control_field<T>(line: &[u8], name: &str) -> io::Result<T>
where
    T: std::str::FromStr,
{
    let line = std::str::from_utf8(line).map_err(|_| invalid_data("control line is not UTF-8"))?;
    line.split_whitespace()
        .find_map(|field| {
            field
                .split_once('=')
                .filter(|(field_name, _)| *field_name == name)
        })
        .map(|(_, value)| value)
        .ok_or_else(|| invalid_data(format!("control field {name} is missing")))?
        .parse()
        .map_err(|_| invalid_data(format!("control field {name} is invalid")))
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
    let mut depth = PixelDepth::Monochrome;
    let mut encoding = None;

    for field in fields.split_whitespace() {
        let Some((name, value)) = field.split_once('=') else {
            return Err(invalid_data("malformed screen header field"));
        };
        match name {
            "width" => width = Some(parse_decimal(value, "width")?),
            "height" => height = Some(parse_decimal(value, "height")?),
            "bytes" => length = Some(parse_decimal(value, "bytes")?),
            "bpp" => {
                depth = PixelDepth::from_bits(parse_decimal(value, "bpp")?)
                    .ok_or_else(|| invalid_data("unsupported screen pixel depth"))?
            }
            "encoding" => encoding = Some(value),
            "crc32" => {
                crc32 = Some(
                    u32::from_str_radix(value, 16)
                        .map_err(|_| invalid_data("invalid screen crc32"))?,
                )
            }
            _ => {}
        }
    }

    if encoding.is_some_and(|value| value != "planar")
        || (depth != PixelDepth::Monochrome && encoding != Some("planar"))
    {
        return Err(invalid_data("unsupported or missing screen encoding"));
    }
    Ok(ScreenHeader {
        depth,
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

fn encode_frame_png(
    frame: &[u8],
    width: u32,
    height: u32,
    depth: PixelDepth,
) -> io::Result<Vec<u8>> {
    let size = Size::new(width as usize, height as usize)
        .map_err(|_| invalid_data("invalid frame dimensions"))?;
    let bitmap = PackedBitmap::new(size, depth, frame)
        .map_err(|_| invalid_data("frame dimensions do not match packed bytes"))?;
    let mut pixels = Vec::with_capacity(width as usize * height as usize);
    for y in 0..size.height() {
        for x in 0..size.width() {
            pixels.push(bitmap.luma(x, y));
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
    use std::{
        ffi::OsString,
        io::Cursor,
        path::{Path, PathBuf},
        time::Duration,
    };

    use image::GenericImageView;

    use super::{
        Arguments, Command, ControlStream, DEFAULT_TIMEOUT, FRAME_BYTES, FRAME_HEIGHT, FRAME_WIDTH,
        ScreenHeader, device_image_name, encode_frame_png, parse_arguments, parse_screen_header,
        transcode_image, validate_device_image,
    };
    use brewthink::{image::PixelDepth, image_decoder::ImageFormat, input::Button};

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
        assert_eq!(
            parse_arguments(["put-image".into(), "sleep.png".into()], None).unwrap(),
            Arguments {
                port: None,
                timeout: DEFAULT_TIMEOUT,
                command: Command::PutImage(PathBuf::from("sleep.png")),
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
                depth: PixelDepth::Monochrome,
                width: FRAME_WIDTH,
                height: FRAME_HEIGHT,
                length: FRAME_BYTES,
                crc32: 0xe4e7_3d7c,
            }
        );
    }

    #[test]
    fn screenshots_decode_all_gray_planes_and_reject_unknown_formats() {
        use brewthink::image::{PackedImage, Size};
        let depth = PixelDepth::Four;

        let bpp = depth.bits();
        let header = format!(
            "BREWCTL/1 SCREEN width=480 height=800 bytes={} crc32=12345678 bpp={bpp} encoding=planar",
            FRAME_BYTES * bpp
        );
        assert_eq!(parse_screen_header(header.as_bytes()).unwrap().depth, depth);
        let size = Size::new(8, 1).unwrap();
        let mut bytes = vec![0; bpp];
        let mut image = PackedImage::new(size, depth, &mut bytes).unwrap();
        let expected: Vec<_> = (0..8)
            .map(|x| {
                ((x % usize::from(depth.levels())) * 255 / usize::from(depth.levels() - 1)) as u8
            })
            .collect();
        for (x, &luma) in expected.iter().enumerate() {
            image.set_luma(x, 0, luma);
        }
        let png = encode_frame_png(&bytes, 8, 1, depth).unwrap();
        assert_eq!(
            image::load_from_memory(&png).unwrap().to_luma8().as_raw(),
            &expected
        );

        for suffix in [
            "bpp=3 encoding=planar",
            "bpp=4 encoding=planar",
            "bpp=2",
            "bpp=3 encoding=interleaved",
        ] {
            let header = format!(
                "BREWCTL/1 SCREEN width=480 height=800 bytes=96000 crc32=12345678 {suffix}"
            );
            assert!(parse_screen_header(header.as_bytes()).is_err());
        }
    }

    #[test]
    fn rewrites_external_image_names_to_deterministic_fat_names() {
        assert_eq!(
            device_image_name(Path::new("nice.png"), ImageFormat::Png)
                .unwrap()
                .as_str(),
            "NICE.PNG"
        );
        let long = device_image_name(Path::new("Summer Holiday Portrait.jpeg"), ImageFormat::Jpeg)
            .unwrap();
        assert!(long.as_str().starts_with("SUM~"));
        assert!(long.as_str().ends_with(".JPG"));
        assert_eq!(
            long,
            device_image_name(Path::new("Summer Holiday Portrait.jpeg"), ImageFormat::Jpeg,)
                .unwrap()
        );
        assert!(
            device_image_name(Path::new("CON.png"), ImageFormat::Png)
                .unwrap()
                .as_str()
                .starts_with("CON~")
        );
    }

    #[test]
    fn validates_images_with_the_device_decoder() {
        validate_device_image(
            ImageFormat::Jpeg,
            include_bytes!("../web/tests/fixtures/cover.jpg"),
        )
        .unwrap();
        validate_device_image(
            ImageFormat::Png,
            include_bytes!("../web/tests/fixtures/transparent.png"),
        )
        .unwrap();
        assert!(validate_device_image(ImageFormat::Jpeg, b"\xff\xd8broken").is_err());
    }

    #[test]
    fn transcodes_sources_into_bounded_baseline_jpegs() {
        let prepared = transcode_image(
            std::path::Path::new("sample.png"),
            include_bytes!("../web/tests/fixtures/transparent.png"),
        )
        .unwrap();
        assert_eq!(prepared.name.as_str(), "SAMPLE.JPG");
        assert!(prepared.transcoded);
        assert!(prepared.bytes.len() <= super::MAX_IMAGE_BYTES);
        validate_device_image(ImageFormat::Jpeg, &prepared.bytes).unwrap();
    }

    #[test]
    fn encodes_packed_pixels_as_png() {
        let frame = vec![0xaa; FRAME_BYTES];
        let encoded =
            encode_frame_png(&frame, FRAME_WIDTH, FRAME_HEIGHT, PixelDepth::Monochrome).unwrap();
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
