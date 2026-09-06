use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    time::{Duration, Instant},
};

use brewthink::storage::{
    Sector,
    diagnostic::{MAX_READ_SECTORS, SectorRange},
};

use super::{Connection, invalid_data, invalid_input, parse_control_field, print_control_line};

pub fn info(connection: &mut Connection, timeout: Duration) -> io::Result<u64> {
    connection.drain()?;
    let deadline = Instant::now() + timeout;
    connection.write_all(b"BREWCTL/1 sd-info\n", deadline)?;
    let mut blocks = None;
    loop {
        let line = connection.read_line(deadline)?;
        if line.starts_with(b"BREWCTL/1 SDINFO ") {
            let count: u64 = parse_control_field(&line, "blocks")?;
            let block_bytes: usize = parse_control_field(&line, "block_bytes")?;
            let readonly: u8 = parse_control_field(&line, "readonly")?;
            if count == 0
                || count > u64::from(u32::MAX) + 1
                || block_bytes != Sector::LEN
                || readonly != 1
            {
                return Err(invalid_data(
                    "unsupported SD geometry or writable diagnostic",
                ));
            }
            if blocks.replace(count).is_some() {
                return Err(invalid_data("duplicate SD information"));
            }
            print_control_line(&line);
        } else if line.starts_with(b"BREWCTL/1 ERROR ") {
            return Err(invalid_data(String::from_utf8_lossy(&line)));
        } else if line.starts_with(b"BREWCTL/1 DONE command=sd-info ") {
            if !line.ends_with(b"status=ok") {
                return Err(invalid_data(String::from_utf8_lossy(&line)));
            }
            return blocks.ok_or_else(|| invalid_data("missing SD information"));
        }
    }
}

pub fn export(
    connection: &mut Connection,
    start: u32,
    count: u32,
    output: &Path,
    timeout: Duration,
) -> io::Result<()> {
    if count == 0 || u64::from(start) + u64::from(count) > u64::from(u32::MAX) + 1 {
        return Err(invalid_input(
            "SD range is empty or overflows sector addressing",
        ));
    }
    if output.try_exists()? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "refusing to overwrite SD export",
        ));
    }
    let blocks = info(connection, timeout)?;
    if u64::from(start) + u64::from(count) > blocks {
        return Err(invalid_input("requested range exceeds card capacity"));
    }
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let mut partial_name = output.as_os_str().to_os_string();
    partial_name.push(".partial");
    let partial = std::path::PathBuf::from(partial_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&partial)?;
    let mut received = 0;
    let mut digest = crc32fast::Hasher::new();
    while received < count {
        let chunk_count = (count - received).min(MAX_READ_SECTORS);
        let range =
            SectorRange::new(start + received, chunk_count).expect("export range was checked");
        let bytes = read_chunk(connection, range, timeout)?;
        file.write_all(&bytes)?;
        digest.update(&bytes);
        received += chunk_count;
    }
    file.sync_all()?;
    drop(file);
    fs::hard_link(&partial, output)?;
    fs::remove_file(&partial)?;
    println!(
        "host: SD export start={start} count={count} bytes={} crc32={:08x} output={}",
        u64::from(count) * Sector::LEN as u64,
        digest.finalize(),
        output.display()
    );
    Ok(())
}

fn read_chunk(
    connection: &mut Connection,
    range: SectorRange,
    timeout: Duration,
) -> io::Result<Vec<u8>> {
    let deadline = Instant::now() + timeout;
    connection.write_all(
        format!("BREWCTL/1 sd-read {} {}\n", range.start(), range.count()).as_bytes(),
        deadline,
    )?;
    let expected_crc = loop {
        let line = connection.read_line(deadline)?;
        if line.starts_with(b"BREWCTL/1 SDREAD ") {
            break parse_chunk_header(&line, range)?;
        }
        if line.starts_with(b"BREWCTL/1 ERROR ") || line.starts_with(b"BREWCTL/1 DONE ") {
            return Err(invalid_data(String::from_utf8_lossy(&line)));
        }
    };
    let bytes = connection.read_payload(range.byte_length(), deadline)?;
    if crc32fast::hash(&bytes) != expected_crc {
        return Err(invalid_data("SD export checksum mismatch"));
    }
    loop {
        let line = connection.read_line(deadline)?;
        if line.starts_with(b"BREWCTL/1 DONE command=sd-read ") && line.ends_with(b"status=ok") {
            return Ok(bytes);
        }
        if line.starts_with(b"BREWCTL/1 ERROR ") || line.starts_with(b"BREWCTL/1 DONE ") {
            return Err(invalid_data(String::from_utf8_lossy(&line)));
        }
    }
}

fn parse_chunk_header(line: &[u8], range: SectorRange) -> io::Result<u32> {
    let start: u32 = parse_control_field(line, "start")?;
    let count: u32 = parse_control_field(line, "count")?;
    let length: usize = parse_control_field(line, "bytes")?;
    if start != range.start() || count != range.count() || length != range.byte_length() {
        return Err(invalid_data(
            "SD response does not match requested sector range",
        ));
    }
    let crc: String = parse_control_field(line, "crc32")?;
    if crc.len() != 8 {
        return Err(invalid_data("invalid SD checksum"));
    }
    u32::from_str_radix(&crc, 16).map_err(|_| invalid_data("invalid SD checksum"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        os::{fd::OwnedFd, unix::net::UnixStream},
        thread,
    };

    fn mock_connection(
        reply: impl FnOnce(UnixStream) + Send + 'static,
    ) -> (Connection, thread::JoinHandle<()>) {
        let (client, server) = UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        server
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let worker = thread::spawn(move || reply(server));
        (
            Connection {
                file: std::fs::File::from(OwnedFd::from(client)),
                stream: Default::default(),
            },
            worker,
        )
    }

    #[test]
    fn sector_response_must_match_the_entire_request() {
        let range = SectorRange::new(32, 2).unwrap();
        assert_eq!(
            parse_chunk_header(
                b"BREWCTL/1 SDREAD start=32 count=2 bytes=1024 crc32=12345678",
                range
            )
            .unwrap(),
            0x12345678
        );
        for line in [
            "start=31 count=2 bytes=1024 crc32=12345678",
            "start=32 count=1 bytes=1024 crc32=12345678",
            "start=32 count=2 bytes=4294967295 crc32=12345678",
            "start=32 count=2 bytes=1024 crc32=xyz",
        ] {
            assert!(parse_chunk_header(line.as_bytes(), range).is_err());
        }
    }

    #[test]
    fn reads_binary_sectors_with_protocol_markers_in_the_payload() {
        let mut bytes = vec![0xA5; Sector::LEN];
        bytes[..12].copy_from_slice(b"BREWCTL/1 \r\n");
        let expected = bytes.clone();
        let (mut connection, worker) = mock_connection(move |mut server| {
            let mut request = String::new();
            BufReader::new(server.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            assert_eq!(request, "BREWCTL/1 sd-read 9 1\n");
            writeln!(
                server,
                "BREWCTL/1 SDREAD start=9 count=1 bytes=512 crc32={:08x}",
                crc32fast::hash(&bytes)
            )
            .unwrap();
            for piece in bytes.chunks(17) {
                server.write_all(piece).unwrap();
            }
            server
                .write_all(b"\nBREWCTL/1 DONE command=sd-read status=ok\n")
                .unwrap();
        });
        assert_eq!(
            read_chunk(
                &mut connection,
                SectorRange::new(9, 1).unwrap(),
                Duration::from_secs(2)
            )
            .unwrap(),
            expected
        );
        worker.join().unwrap();
    }

    #[test]
    fn rejects_corrupt_truncated_failed_and_missing_payloads() {
        for response in [
            [
                b"BREWCTL/1 SDREAD start=0 count=1 bytes=512 crc32=00000000\n".as_slice(),
                &[0; 512],
                b"\nBREWCTL/1 DONE command=sd-read status=ok\n",
            ]
            .concat(),
            b"BREWCTL/1 SDREAD start=0 count=1 bytes=512 crc32=00000000\nshort".to_vec(),
            b"BREWCTL/1 ERROR command=sd-read reason=out-of-range\n".to_vec(),
            b"BREWCTL/1 DONE command=sd-read status=ok\n".to_vec(),
        ] {
            let (mut connection, worker) = mock_connection(move |mut server| {
                let mut request = [0; 128];
                let _ = server.read(&mut request).unwrap();
                server.write_all(&response).unwrap();
            });
            assert!(
                read_chunk(
                    &mut connection,
                    SectorRange::new(0, 1).unwrap(),
                    Duration::from_secs(2)
                )
                .is_err()
            );
            worker.join().unwrap();
        }
    }
}
