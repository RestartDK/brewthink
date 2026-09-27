mod support;
use std::ffi::CStr;
use std::{
    fs,
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};
use support::*;

type Settings = (
    libc::tcflag_t,
    libc::tcflag_t,
    libc::tcflag_t,
    libc::tcflag_t,
    Vec<libc::cc_t>,
    libc::speed_t,
    libc::speed_t,
);
fn settings(fd: &OwnedFd) -> Settings {
    let mut state = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: fd remains owned for the call and tcgetattr initializes the complete termios value on success.
    let state = unsafe {
        assert_eq!(libc::tcgetattr(fd.as_raw_fd(), state.as_mut_ptr()), 0);
        state.assume_init()
    };
    // PENDIN is pending kernel input processing, not a terminal configuration setting.
    // SAFETY: both speed accessors receive the initialized termios value above.
    unsafe {
        (
            state.c_iflag,
            state.c_oflag,
            state.c_cflag,
            state.c_lflag & !libc::PENDIN,
            state.c_cc.to_vec(),
            libc::cfgetispeed(&state),
            libc::cfgetospeed(&state),
        )
    }
}
struct Pty {
    master: fs::File,
    slave: OwnedFd,
    path: PathBuf,
}
impl Pty {
    fn new() -> Self {
        let _creation = PROCESS_CREATION.lock().unwrap();
        let (mut master, mut slave) = (-1, -1);
        // SAFETY: openpty receives valid descriptor outputs; null pointers request default terminal settings.
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0
        );
        // SAFETY: successful openpty returned two distinct, newly owned descriptors.
        let (master, slave) =
            unsafe { (fs::File::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
            // SAFETY: these descriptors are owned here; concurrent command creation is locked out until CLOEXEC is set.
            unsafe {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                assert!(flags >= 0);
                assert_eq!(libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC), 0);
            }
        }
        let mut name = [0; 1024];
        // SAFETY: the live slave and writable buffer remain valid throughout ttyname_r.
        assert_eq!(
            unsafe { libc::ttyname_r(slave.as_raw_fd(), name.as_mut_ptr(), name.len()) },
            0
        );
        // SAFETY: successful ttyname_r wrote a NUL-terminated pathname into name.
        let path = PathBuf::from(unsafe { CStr::from_ptr(name.as_ptr()) }.to_str().unwrap());
        // SAFETY: fcntl operates only on the owned test PTY master.
        unsafe {
            let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
            assert!(flags >= 0);
            assert_eq!(
                libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK),
                0
            );
        }
        Self {
            master,
            slave,
            path,
        }
    }
}
#[test]
fn pty_descriptors_do_not_cross_exec() {
    let pty = Pty::new();
    let duplicate = pty.master.try_clone().unwrap();
    for fd in [
        pty.master.as_raw_fd(),
        pty.slave.as_raw_fd(),
        duplicate.as_raw_fd(),
    ] {
        // SAFETY: all three descriptors remain owned and open during the query.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0);
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
    }
}

struct Peer {
    file: fs::File,
    buffer: Vec<u8>,
    deadline: Instant,
}
impl Peer {
    fn new(file: fs::File) -> Self {
        Self {
            file,
            buffer: vec![],
            deadline: Instant::now() + Duration::from_secs(15),
        }
    }
    fn wait(&self, writable: bool) {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "PTY peer timed out");
        let mut poll = libc::pollfd {
            fd: self.file.as_raw_fd(),
            events: if writable {
                libc::POLLOUT
            } else {
                libc::POLLIN
            },
            revents: 0,
        };
        // SAFETY: poll receives one initialized descriptor record referencing the owned PTY master.
        let count = unsafe {
            libc::poll(
                &mut poll,
                1,
                remaining.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        assert!(
            count > 0,
            "PTY poll failed or timed out: {}",
            std::io::Error::last_os_error()
        );
        assert_ne!(
            poll.revents & poll.events,
            0,
            "PTY closed: {}",
            poll.revents
        );
    }
    fn write(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            self.wait(true);
            match self.file.write(&bytes[..bytes.len().min(16384)]) {
                Ok(n) => {
                    assert!(n > 0);
                    bytes = &bytes[n..];
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => panic!("PTY write: {e}"),
            }
        }
    }
    fn fill(&mut self) {
        self.wait(false);
        let mut data = [0; 16384];
        match self.file.read(&mut data) {
            Ok(n) => {
                assert!(n > 0);
                self.buffer.extend_from_slice(&data[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("PTY read: {e}"),
        }
    }
    fn read(&mut self, count: usize) -> Vec<u8> {
        while self.buffer.len() < count {
            self.fill()
        }
        self.buffer.drain(..count).collect()
    }
    fn line(&mut self) -> Vec<u8> {
        loop {
            if let Some(i) = self.buffer.iter().position(|b| *b == b'\n') {
                return self.read(i + 1);
            }
            self.fill();
        }
    }
}
fn tools() -> &'static Path {
    static TOOLS: OnceLock<PathBuf> = OnceLock::new();
    TOOLS
        .get_or_init(|| {
            let version = success(run(
                Command::new("rustc").arg("-vV"),
                Duration::from_secs(10),
            ));
            let version = String::from_utf8(version.stdout).unwrap();
            let host = version
                .lines()
                .find_map(|line| line.strip_prefix("host: "))
                .unwrap();
            let target = root().join("target/host-cli-tests");
            success(run(
                Command::new("cargo").current_dir(root()).args([
                    "build",
                    "--locked",
                    "--target",
                    host,
                    "--target-dir",
                    target.to_str().unwrap(),
                    "--features",
                    "device-control,host-image-tools",
                    "--bin",
                    "device-control",
                    "--bin",
                    "prepare-image",
                ]),
                Duration::from_secs(600),
            ));
            target.join(host).join("debug")
        })
        .as_path()
}
fn client(args: &[&str], serve: impl FnOnce(&mut Peer) + Send + 'static) -> Output {
    let binary = tools().join("device-control");
    let pty = Pty::new();
    let original = settings(&pty.slave);
    let peer_master = pty.master.try_clone().unwrap();
    let server = thread::spawn(move || serve(&mut Peer::new(peer_master)));
    let result = run(
        Command::new(binary)
            .args(["--port", pty.path.to_str().unwrap(), "--timeout", "10"])
            .args(args),
        Duration::from_secs(20),
    );
    assert!(
        server.join().is_ok(),
        "peer failed\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(settings(&pty.slave), original);
    result
}
fn prepare(
    source: &Path,
    packed: &Path,
    preview: &Path,
    width: &str,
    height: &str,
    quantizer: &str,
) -> Output {
    run(
        Command::new(tools().join("prepare-image"))
            .arg(source)
            .arg(packed)
            .arg(preview)
            .args([width, height, "contain", quantizer]),
        Duration::from_secs(20),
    )
}
#[test]
fn screenshots_round_trip_legacy_and_four_tones() {
    for (bits, quantizer) in [(1, "threshold"), (2, "gray4")] {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("screen.png");
        let packed = dir.path().join("frame.bin");
        let preview = dir.path().join("preview.pnm");
        let frame: Vec<_> = [0x55, 0x33, 0x0f][..bits]
            .iter()
            .flat_map(|v| vec![*v; 48000])
            .collect();
        let expected = frame.clone();
        success(client(&["screen", output.to_str().unwrap()], move |peer| {
            assert_eq!(peer.line(), b"BREWCTL/1 screen\n");
            let extra = if bits == 1 {
                String::new()
            } else {
                format!(" bpp={bits} encoding=planar")
            };
            peer.write(
                format!(
                    "BREWCTL/1 SCREEN width=480 height=800 bytes={} crc32={:08x}{extra}\n",
                    frame.len(),
                    crc32fast::hash(&frame)
                )
                .as_bytes(),
            );
            peer.write(&frame);
            peer.write(b"\nBREWCTL/1 DONE command=screen status=ok\n");
        }));
        success(prepare(&output, &packed, &preview, "480", "800", quantizer));
        assert_eq!(fs::read(packed).unwrap(), expected);
    }
}
#[test]
fn prepare_composites_alpha_and_writes_four_level_pgm() {
    let dir = tempfile::tempdir().unwrap();
    let packed = dir.path().join("frame.bin");
    let preview = dir.path().join("preview.pgm");
    success(prepare(
        &root().join("web/tests/fixtures/gray-ramp.png"),
        &packed,
        &preview,
        "64",
        "16",
        "gray4",
    ));
    assert_eq!(fs::read(packed).unwrap().len(), 256);
    let pgm = fs::read(preview).unwrap();
    let header = b"P5\n64 16\n255\n";
    assert!(pgm.starts_with(header));
    let row: Vec<_> = [0, 85, 170, 255]
        .iter()
        .flat_map(|v| vec![*v; 16])
        .collect();
    assert_eq!(&pgm[header.len()..], row.repeat(16));
}
#[test]
fn bad_screen_checksum_never_writes_a_png() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("screen.png");
    let result = client(&["screen", output.to_str().unwrap()], |peer| {
        assert_eq!(peer.line(), b"BREWCTL/1 screen\n");
        let frame = vec![0; 96000];
        peer.write(format!("BREWCTL/1 SCREEN width=480 height=800 bytes=96000 crc32={:08x} bpp=2 encoding=planar\n",crc32fast::hash(&frame)^1).as_bytes());
        peer.write(&frame);
    });
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("checksum mismatch"));
    assert!(!output.exists());
}
#[test]
fn upload_transcodes_to_the_image_limit() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("ramp.png");
    let mut data = fs::read(root().join("web/tests/fixtures/gray-ramp.png")).unwrap();
    data.extend(vec![0; 100 * 1024]);
    fs::write(&source, data).unwrap();
    let result = success(client(&["put-image", source.to_str().unwrap()], |peer| {
        let command = String::from_utf8(peer.line()).unwrap();
        let command: Vec<_> = command.split_whitespace().collect();
        assert_eq!(command[..3], ["BREWCTL/1", "upload", "image"]);
        assert_eq!(command[3], "RAMP.JPG");
        let length: usize = command[4].parse().unwrap();
        assert!(length <= 96 * 1024);
        peer.write(
            format!("BREWCTL/1 READY command=upload chunk=4096 bytes={length}\n").as_bytes(),
        );
        let mut received = vec![];
        while received.len() < length {
            received.extend(peer.read(4096.min(length - received.len())));
            peer.write(
                format!("BREWCTL/1 ACK command=upload received={}\n", received.len()).as_bytes(),
            );
        }
        assert!(received.starts_with(&[0xff, 0xd8]));
        assert_eq!(
            crc32fast::hash(&received),
            u32::from_str_radix(command[5], 16).unwrap()
        );
        peer.write(b"BREWCTL/1 DONE command=upload status=ok\n");
    }));
    assert!(String::from_utf8_lossy(&result.stdout).contains("transcoded=yes"));
}
fn receive_book(peer: &mut Peer, expected: &[u8], name: &str) {
    let command = String::from_utf8(peer.line()).unwrap();
    let fields: Vec<_> = command.split_whitespace().collect();
    assert_eq!(fields[..3], ["BREWCTL/1", "upload", "book"]);
    assert_eq!(fields[3], name);
    assert_eq!(fields[4].parse::<usize>().unwrap(), expected.len());
    assert_eq!(
        u32::from_str_radix(fields[5], 16).unwrap(),
        crc32fast::hash(expected)
    );
    peer.write(
        format!(
            "BREWCTL/1 READY command=upload chunk=4096 bytes={}\n",
            expected.len()
        )
        .as_bytes(),
    );
    let mut received = Vec::new();
    while received.len() < expected.len() {
        received.extend(peer.read(4096.min(expected.len() - received.len())));
        peer.write(
            format!("BREWCTL/1 ACK command=upload received={}\n", received.len()).as_bytes(),
        );
    }
    assert_eq!(received, expected);
}

#[test]
fn book_upload_streams_unchanged_epubs_larger_than_the_image_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library book with a long filename.epub");
    let bytes = fs::read(root().join("web/tests/fixtures/parity/shelf-limit.epub")).unwrap();
    assert!(bytes.len() > 96 * 1024);
    fs::write(&path, &bytes).unwrap();
    let name = format!("{:08X}.EPB", crc32fast::hash(&bytes));
    let result = client(&["put-book", path.to_str().unwrap()], move |peer| {
        receive_book(peer, &bytes, &name);
        peer.write(b"BREWCTL/1 DONE command=upload status=ok\n");
    });
    success(result);
}

#[test]
fn batch_books_waits_for_each_verified_commit() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = fs::read(root().join("web/tests/fixtures/minimal.epub")).unwrap();
    fs::write(dir.path().join("FIRST.epub"), &bytes).unwrap();
    fs::write(dir.path().join("SECOND.EPUB"), &bytes).unwrap();
    fs::write(dir.path().join("ignored.txt"), b"not a book").unwrap();
    success(client(
        &["put-books", dir.path().to_str().unwrap()],
        move |peer| {
            for name in ["FIRST.EPB", "SECOND.EPB"] {
                receive_book(peer, &bytes, name);
                peer.write(b"BREWCTL/1 DONE command=upload status=ok\n");
            }
        },
    ));
}

#[test]
fn book_upload_reports_device_rejection_and_failed_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("BOOK.epub");
    let bytes = fs::read(root().join("web/tests/fixtures/minimal.epub")).unwrap();
    fs::write(&path, &bytes).unwrap();
    let rejected = client(&["put-book", path.to_str().unwrap()], |peer| {
        assert!(peer.line().starts_with(b"BREWCTL/1 upload book "));
        peer.write(b"BREWCTL/1 ERROR command=parse reason=invalid-upload\nBREWCTL/1 DONE command=parse status=error\n");
    });
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("device rejected"));
    let failed = client(&["put-book", path.to_str().unwrap()], move |peer| {
        receive_book(peer, &bytes, "BOOK.EPB");
        peer.write(b"BREWCTL/1 ERROR command=upload reason=checksum-mismatch\nBREWCTL/1 DONE command=upload status=error\n");
    });
    assert!(!failed.status.success());
}

#[test]
fn verify_book_checks_existing_bytes_without_sending_a_payload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("BOOK.epub");
    let bytes = fs::read(root().join("web/tests/fixtures/minimal.epub")).unwrap();
    fs::write(&path, &bytes).unwrap();
    let expected = format!(
        "BREWCTL/1 verify book BOOK.EPB {} {:08x}\n",
        bytes.len(),
        crc32fast::hash(&bytes)
    );
    for status in ["ok", "error"] {
        let expected = expected.clone();
        let result = client(&["verify-book", path.to_str().unwrap()], move |peer| {
            assert_eq!(peer.line(), expected.as_bytes());
            peer.write(format!("BREWCTL/1 DONE command=verify status={status}\n").as_bytes());
        });
        assert_eq!(result.status.success(), status == "ok");
    }
}

#[test]
fn books_preflight_never_opens_a_device_and_rejects_invalid_archives() {
    let dir = tempfile::tempdir().unwrap();
    fs::copy(
        root().join("web/tests/fixtures/parity/oversized-chapter.epub"),
        dir.path().join("BOOK.epub"),
    )
    .unwrap();
    let result = success(run(
        Command::new(tools().join("device-control")).args([
            "--port",
            "/no-device",
            "check-books",
            dir.path().to_str().unwrap(),
        ]),
        Duration::from_secs(20),
    ));
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("streamed text checks"));
    assert!(!stdout.contains("reader warning"));
    fs::copy(
        root().join("web/tests/fixtures/parity/malformed.epub"),
        dir.path().join("WARN.epub"),
    )
    .unwrap();
    let result = success(run(
        Command::new(tools().join("device-control")).args([
            "--port",
            "/no-device",
            "check-books",
            dir.path().to_str().unwrap(),
        ]),
        Duration::from_secs(20),
    ));
    assert!(String::from_utf8_lossy(&result.stdout).contains("Malformed"));
    fs::write(dir.path().join("BAD.epub"), b"PK\x03\x04broken").unwrap();
    let result = run(
        Command::new(tools().join("device-control")).args([
            "--port",
            "/no-device",
            "put-books",
            dir.path().to_str().unwrap(),
        ]),
        Duration::from_secs(20),
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("InvalidZip"));
}

fn exchange(reply: &[u8], command: &str) -> Output {
    let pty = Pty::new();
    let command = command.to_string();
    let expected = format!("BREWGRAY/1 {command}\n");
    let reply = reply.to_vec();
    let peer_master = pty.master.try_clone().unwrap();
    let server = thread::spawn(move || {
        let mut peer = Peer::new(peer_master);
        assert_eq!(peer.line(), expected.as_bytes());
        peer.write(&reply);
    });
    let result = run(
        Command::new("python3")
            .arg(root().join("tools/grayscale-bench.py"))
            .args([
                "--port",
                pty.path.to_str().unwrap(),
                "--timeout",
                "1",
                &command,
            ]),
        Duration::from_secs(5),
    );
    assert!(
        server.join().is_ok(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    drop(pty.slave);
    result
}
#[test]
fn requires_matching_terminal_response() {
    let result=success(exchange(b"unrelated startup log\nBREWGRAY/1 DONE command=status status=ok\nBREWGRAY/1 START command=four attempt=1\nBREWGRAY/1 DONE command=four status=ok optical=unmeasured\n","four"));
    assert!(String::from_utf8_lossy(&result.stdout).contains("optical=unmeasured"));
}
#[test]
fn blocked_and_error_are_not_success() {
    for status in ["blocked", "error"] {
        assert!(
            !exchange(
                format!("BREWGRAY/1 DONE command=four status={status}\n").as_bytes(),
                "four"
            )
            .status
            .success()
        );
    }
}
#[test]
fn other_commands_do_not_complete_request() {
    let result = exchange(b"BREWGRAY/1 DONE command=status status=ok\n", "four");
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("no retry attempted"));
}
#[test]
fn transition_probe_has_an_explicit_name() {
    let result = success(exchange(
        b"BREWGRAY/1 DONE command=sixteen-probe status=ok optical=unmeasured\n",
        "sixteen-probe",
    ));
    assert!(String::from_utf8_lossy(&result.stdout).contains("optical=unmeasured"));
}
#[test]
fn eight_repeat_has_an_explicit_name() {
    let result = success(exchange(
        b"BREWGRAY/1 DONE command=eight-repeat status=ok optical=unmeasured\n",
        "eight-repeat",
    ));
    assert!(String::from_utf8_lossy(&result.stdout).contains("optical=unmeasured"));
}
#[test]
fn unknown_depth_rejected_before_opening_device() {
    let result = run(
        Command::new("python3")
            .arg(root().join("tools/grayscale-bench.py"))
            .args(["--port", "/not/a/device", "eight"]),
        Duration::from_secs(5),
    );
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid choice"));
}
