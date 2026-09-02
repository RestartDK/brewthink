use crc32fast::Hasher;
use miniz_oxide::{
    DataFormat, MZFlush, MZStatus,
    inflate::stream::{InflateState, inflate},
};

pub const MAX_ZIP_ENTRIES: usize = 1_024;
pub const MAX_ZIP_PATH_BYTES: usize = 256;
pub const MAX_INFLATION_RATIO: u32 = 200;

const CENTRAL_HEADER: u32 = 0x0201_4B50;
const LOCAL_HEADER: u32 = 0x0403_4B50;
const END_OF_CENTRAL_DIRECTORY: u32 = 0x0605_4B50;
const CENTRAL_HEADER_BYTES: usize = 46;
const LOCAL_HEADER_BYTES: usize = 30;
const EOCD_BYTES: usize = 22;
const MAX_EOCD_SEARCH: u32 = u16::MAX as u32 + EOCD_BYTES as u32;
const COMPRESSION_STORED: u16 = 0;
const COMPRESSION_DEFLATE: u16 = 8;
const FLAG_ENCRYPTED: u16 = 1;

pub trait ReadAt {
    type Error;

    fn len(&self) -> u32;
    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZipPath {
    bytes: [u8; MAX_ZIP_PATH_BYTES],
    length: u16,
}

impl ZipPath {
    fn from_bytes(bytes: &[u8]) -> Result<Self, ZipError<core::convert::Infallible>> {
        let value = core::str::from_utf8(bytes).map_err(|_| ZipError::InvalidPath)?;
        if !safe_path(value) || bytes.len() > MAX_ZIP_PATH_BYTES {
            return Err(ZipError::InvalidPath);
        }
        let mut path = Self {
            bytes: [0; MAX_ZIP_PATH_BYTES],
            length: bytes.len() as u16,
        };
        path.bytes[..bytes.len()].copy_from_slice(bytes);
        Ok(path)
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..usize::from(self.length)])
            .expect("ZIP paths are checked as UTF-8 when parsed")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZipEntry {
    path: ZipPath,
    compression: u16,
    flags: u16,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    local_header_offset: u32,
}

impl ZipEntry {
    pub const fn path(&self) -> &ZipPath {
        &self.path
    }

    pub const fn compressed_size(self) -> u32 {
        self.compressed_size
    }

    pub const fn uncompressed_size(self) -> u32 {
        self.uncompressed_size
    }

    pub const fn is_stored(self) -> bool {
        self.compression == COMPRESSION_STORED
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ZipError<E> {
    Read(E),
    Truncated,
    MissingCentralDirectory,
    MultiDisk,
    Zip64Unsupported,
    TooManyEntries,
    InvalidCentralDirectory,
    InvalidLocalHeader,
    InvalidPath,
    DuplicatePath,
    Encrypted,
    UnsupportedCompression,
    ExcessiveInflation,
    EntryNotFound,
    OutputTooSmall,
    Decompression,
    ResourceLengthMismatch,
    CrcMismatch,
}

pub struct ZipValidationScratch {
    path_hashes: [u32; MAX_ZIP_ENTRIES],
    length: usize,
}

impl ZipValidationScratch {
    pub const fn new() -> Self {
        Self {
            path_hashes: [0; MAX_ZIP_ENTRIES],
            length: 0,
        }
    }

    fn insert(&mut self, hash: u32) -> bool {
        if self.path_hashes[..self.length].contains(&hash) {
            return false;
        }
        self.path_hashes[self.length] = hash;
        self.length += 1;
        true
    }
}

impl Default for ZipValidationScratch {
    fn default() -> Self {
        Self::new()
    }
}

pub struct InflateWorkspace {
    state: InflateState,
}

impl InflateWorkspace {
    pub fn new() -> Self {
        Self {
            state: InflateState::new(DataFormat::Raw),
        }
    }

    #[cfg(any(test, target_arch = "riscv32"))]
    pub(crate) unsafe fn initialize_in_place(storage: *mut Self) {
        // SAFETY: miniz_oxide 0.9.1's InflateState is valid when zeroed: its
        // decompressor documents an all-zero initial state, and zero is a valid
        // discriminant for its remaining enums and booleans. Reset establishes
        // the required Raw format and streaming flags before first use.
        unsafe {
            storage.write_bytes(0, 1);
            (*storage).state.reset(DataFormat::Raw);
        }
    }

    fn reset(&mut self) {
        self.state.reset(DataFormat::Raw);
    }
}

impl Default for InflateWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

pub struct StreamingZip<R> {
    reader: R,
    central_offset: u32,
    central_size: u32,
    entry_count: u16,
}

impl<R> StreamingZip<R>
where
    R: ReadAt,
{
    pub fn open(reader: R, scratch: &mut ZipValidationScratch) -> Result<Self, ZipError<R::Error>> {
        let (eocd_offset, eocd) = find_eocd(&reader)?;
        if le_u16(&eocd, 4) != 0 || le_u16(&eocd, 6) != 0 {
            return Err(ZipError::MultiDisk);
        }
        let entries_on_disk = le_u16(&eocd, 8);
        let entry_count = le_u16(&eocd, 10);
        let central_size = le_u32(&eocd, 12);
        let central_offset = le_u32(&eocd, 16);
        if entries_on_disk == u16::MAX
            || entry_count == u16::MAX
            || central_size == u32::MAX
            || central_offset == u32::MAX
        {
            return Err(ZipError::Zip64Unsupported);
        }
        if entries_on_disk != entry_count {
            return Err(ZipError::MultiDisk);
        }
        if usize::from(entry_count) > MAX_ZIP_ENTRIES {
            return Err(ZipError::TooManyEntries);
        }
        if central_offset
            .checked_add(central_size)
            .is_none_or(|end| end > eocd_offset)
        {
            return Err(ZipError::InvalidCentralDirectory);
        }

        scratch.length = 0;
        let archive = Self {
            reader,
            central_offset,
            central_size,
            entry_count,
        };
        let mut cursor = central_offset;
        for _ in 0..entry_count {
            let (entry, next) = archive.entry_at(cursor)?;
            if !scratch.insert(path_hash(entry.path.as_str().as_bytes())) {
                return Err(ZipError::DuplicatePath);
            }
            cursor = next;
        }
        if cursor != central_offset + central_size {
            return Err(ZipError::InvalidCentralDirectory);
        }
        Ok(archive)
    }

    pub const fn entry_count(&self) -> usize {
        self.entry_count as usize
    }

    pub fn first_entry(&self) -> Result<ZipEntry, ZipError<R::Error>> {
        if self.entry_count == 0 {
            return Err(ZipError::EntryNotFound);
        }
        self.entry_at(self.central_offset).map(|(entry, _)| entry)
    }

    pub fn find(&self, path: &str) -> Result<ZipEntry, ZipError<R::Error>> {
        let mut cursor = self.central_offset;
        for _ in 0..self.entry_count {
            let (entry, next) = self.entry_at(cursor)?;
            if entry.path.as_str() == path {
                return Ok(entry);
            }
            cursor = next;
        }
        Err(ZipError::EntryNotFound)
    }

    pub fn read_entry(
        &self,
        entry: ZipEntry,
        output: &mut [u8],
        inflate_workspace: &mut InflateWorkspace,
    ) -> Result<usize, ZipError<R::Error>> {
        let output_size =
            usize::try_from(entry.uncompressed_size).map_err(|_| ZipError::OutputTooSmall)?;
        if output_size > output.len() {
            return Err(ZipError::OutputTooSmall);
        }
        let data_offset = self.entry_data_offset(entry)?;
        match entry.compression {
            COMPRESSION_STORED => {
                read_exact(&self.reader, data_offset, &mut output[..output_size])?;
                if entry.compressed_size != entry.uncompressed_size {
                    return Err(ZipError::ResourceLengthMismatch);
                }
            }
            COMPRESSION_DEFLATE => self.inflate_entry(
                data_offset,
                entry.compressed_size,
                &mut output[..output_size],
                inflate_workspace,
            )?,
            _ => return Err(ZipError::UnsupportedCompression),
        }
        let mut hasher = Hasher::new();
        hasher.update(&output[..output_size]);
        if hasher.finalize() != entry.crc32 {
            return Err(ZipError::CrcMismatch);
        }
        Ok(output_size)
    }

    pub fn into_reader(self) -> R {
        self.reader
    }

    fn entry_at(&self, offset: u32) -> Result<(ZipEntry, u32), ZipError<R::Error>> {
        let central_end = self.central_offset + self.central_size;
        if offset
            .checked_add(CENTRAL_HEADER_BYTES as u32)
            .is_none_or(|end| end > central_end)
        {
            return Err(ZipError::InvalidCentralDirectory);
        }
        let mut header = [0; CENTRAL_HEADER_BYTES];
        read_exact(&self.reader, offset, &mut header)?;
        if le_u32(&header, 0) != CENTRAL_HEADER {
            return Err(ZipError::InvalidCentralDirectory);
        }
        let flags = le_u16(&header, 8);
        if flags & FLAG_ENCRYPTED != 0 {
            return Err(ZipError::Encrypted);
        }
        if le_u16(&header, 34) != 0 {
            return Err(ZipError::MultiDisk);
        }
        let compression = le_u16(&header, 10);
        if !matches!(compression, COMPRESSION_STORED | COMPRESSION_DEFLATE) {
            return Err(ZipError::UnsupportedCompression);
        }
        let compressed_size = le_u32(&header, 20);
        let uncompressed_size = le_u32(&header, 24);
        if compressed_size == u32::MAX || uncompressed_size == u32::MAX {
            return Err(ZipError::Zip64Unsupported);
        }
        if compressed_size > 0 && uncompressed_size / compressed_size > MAX_INFLATION_RATIO {
            return Err(ZipError::ExcessiveInflation);
        }
        let name_length = usize::from(le_u16(&header, 28));
        let extra_length = u32::from(le_u16(&header, 30));
        let comment_length = u32::from(le_u16(&header, 32));
        if name_length == 0 || name_length > MAX_ZIP_PATH_BYTES {
            return Err(ZipError::InvalidPath);
        }
        let mut name = [0; MAX_ZIP_PATH_BYTES];
        read_exact(
            &self.reader,
            offset + CENTRAL_HEADER_BYTES as u32,
            &mut name[..name_length],
        )?;
        let path = ZipPath::from_bytes(&name[..name_length]).map_err(map_infallible)?;
        let variable_size = (name_length as u32)
            .checked_add(extra_length)
            .and_then(|size| size.checked_add(comment_length))
            .ok_or(ZipError::InvalidCentralDirectory)?;
        let next = offset
            .checked_add(CENTRAL_HEADER_BYTES as u32)
            .and_then(|value| value.checked_add(variable_size))
            .filter(|next| *next <= central_end)
            .ok_or(ZipError::InvalidCentralDirectory)?;
        Ok((
            ZipEntry {
                path,
                compression,
                flags,
                crc32: le_u32(&header, 16),
                compressed_size,
                uncompressed_size,
                local_header_offset: le_u32(&header, 42),
            },
            next,
        ))
    }

    fn entry_data_offset(&self, entry: ZipEntry) -> Result<u32, ZipError<R::Error>> {
        let mut header = [0; LOCAL_HEADER_BYTES];
        read_exact(&self.reader, entry.local_header_offset, &mut header)?;
        if le_u32(&header, 0) != LOCAL_HEADER {
            return Err(ZipError::InvalidLocalHeader);
        }
        if le_u16(&header, 6) != entry.flags || le_u16(&header, 8) != entry.compression {
            return Err(ZipError::InvalidLocalHeader);
        }
        let name_length = usize::from(le_u16(&header, 26));
        let extra_length = u32::from(le_u16(&header, 28));
        if name_length != entry.path.as_str().len() {
            return Err(ZipError::InvalidLocalHeader);
        }
        let mut name = [0; MAX_ZIP_PATH_BYTES];
        read_exact(
            &self.reader,
            entry.local_header_offset + LOCAL_HEADER_BYTES as u32,
            &mut name[..name_length],
        )?;
        if &name[..name_length] != entry.path.as_str().as_bytes() {
            return Err(ZipError::InvalidLocalHeader);
        }
        entry
            .local_header_offset
            .checked_add(LOCAL_HEADER_BYTES as u32)
            .and_then(|value| value.checked_add(name_length as u32))
            .and_then(|value| value.checked_add(extra_length))
            .filter(|offset| {
                offset
                    .checked_add(entry.compressed_size)
                    .is_some_and(|end| end <= self.reader.len())
            })
            .ok_or(ZipError::InvalidLocalHeader)
    }

    fn inflate_entry(
        &self,
        data_offset: u32,
        compressed_size: u32,
        output: &mut [u8],
        workspace: &mut InflateWorkspace,
    ) -> Result<(), ZipError<R::Error>> {
        workspace.reset();
        let mut input = [0; 512];
        let mut input_length = 0;
        let mut input_position = 0;
        let mut compressed_position = 0u32;
        let mut output_position = 0usize;
        let mut overflow = [0; 1];

        loop {
            if input_position == input_length && compressed_position < compressed_size {
                let remaining = (compressed_size - compressed_position) as usize;
                input_length = remaining.min(input.len());
                read_exact(
                    &self.reader,
                    data_offset + compressed_position,
                    &mut input[..input_length],
                )?;
                compressed_position += input_length as u32;
                input_position = 0;
            }
            let input_slice = &input[input_position..input_length];
            let using_overflow = output_position == output.len();
            let output_slice = if using_overflow {
                &mut overflow[..]
            } else {
                &mut output[output_position..]
            };
            let result = inflate(
                &mut workspace.state,
                input_slice,
                output_slice,
                MZFlush::None,
            );
            input_position += result.bytes_consumed;
            if using_overflow {
                if result.bytes_written != 0 {
                    return Err(ZipError::ResourceLengthMismatch);
                }
            } else {
                output_position += result.bytes_written;
            }
            match result.status {
                Ok(MZStatus::StreamEnd) => {
                    if output_position != output.len()
                        || input_position != input_length
                        || compressed_position != compressed_size
                    {
                        return Err(ZipError::ResourceLengthMismatch);
                    }
                    return Ok(());
                }
                Ok(MZStatus::Ok) => {}
                Ok(_) | Err(_) => return Err(ZipError::Decompression),
            }
            if result.bytes_consumed == 0 && result.bytes_written == 0 {
                return Err(ZipError::Decompression);
            }
        }
    }
}

fn find_eocd<R>(reader: &R) -> Result<(u32, [u8; EOCD_BYTES]), ZipError<R::Error>>
where
    R: ReadAt,
{
    let file_length = reader.len();
    if file_length < EOCD_BYTES as u32 {
        return Err(ZipError::MissingCentralDirectory);
    }
    let lower_bound = file_length.saturating_sub(MAX_EOCD_SEARCH);
    let mut window_end = file_length;
    let mut window = [0; 533];
    loop {
        let window_start = window_end.saturating_sub(512).max(lower_bound);
        let window_length = (window_end - window_start) as usize;
        read_exact(reader, window_start, &mut window[..window_length])?;
        if window_length >= 4 {
            for index in (0..=window_length - 4).rev() {
                if le_u32(&window[..window_length], index) != END_OF_CENTRAL_DIRECTORY {
                    continue;
                }
                let offset = window_start + index as u32;
                if offset + EOCD_BYTES as u32 > file_length {
                    continue;
                }
                let mut eocd = [0; EOCD_BYTES];
                read_exact(reader, offset, &mut eocd)?;
                let comment_length = u32::from(le_u16(&eocd, 20));
                if offset + EOCD_BYTES as u32 + comment_length == file_length {
                    return Ok((offset, eocd));
                }
            }
        }
        if window_start == lower_bound {
            break;
        }
        window_end = (window_start + 21).min(file_length);
    }
    Err(ZipError::MissingCentralDirectory)
}

fn read_exact<R>(reader: &R, offset: u32, output: &mut [u8]) -> Result<(), ZipError<R::Error>>
where
    R: ReadAt,
{
    if offset
        .checked_add(output.len() as u32)
        .is_none_or(|end| end > reader.len())
    {
        return Err(ZipError::Truncated);
    }
    let mut read = 0;
    while read < output.len() {
        let count = reader
            .read_at(offset + read as u32, &mut output[read..])
            .map_err(ZipError::Read)?;
        if count == 0 {
            return Err(ZipError::Truncated);
        }
        read += count;
    }
    Ok(())
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', '\0'])
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

fn path_hash(path: &[u8]) -> u32 {
    path.iter().fold(0x811C_9DC5, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
}

fn map_infallible<E>(error: ZipError<core::convert::Infallible>) -> ZipError<E> {
    match error {
        ZipError::InvalidPath => ZipError::InvalidPath,
        _ => unreachable!("path parsing only returns InvalidPath"),
    }
}

fn le_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn le_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::{boxed::Box, convert::Infallible};

    use super::{
        InflateWorkspace, ReadAt, StreamingZip, ZipError, ZipValidationScratch, safe_path,
    };

    struct SliceFile<'a>(&'a [u8]);

    impl ReadAt for SliceFile<'_> {
        type Error = Infallible;

        fn len(&self) -> u32 {
            self.0.len() as u32
        }

        fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
            let start = offset as usize;
            let available = self.0.len().saturating_sub(start);
            let count = available.min(output.len());
            output[..count].copy_from_slice(&self.0[start..start + count]);
            Ok(count)
        }
    }

    #[test]
    fn finds_and_inflates_entries_without_loading_the_archive() {
        let encoded = include_bytes!("../web/tests/fixtures/minimal.epub");
        let mut scratch = Box::new(ZipValidationScratch::new());
        let archive = StreamingZip::open(SliceFile(encoded), &mut scratch).unwrap();
        assert_eq!(archive.entry_count(), 6);
        assert_eq!(archive.first_entry().unwrap().path().as_str(), "mimetype");

        let entry = archive.find("EPUB/chapter.xhtml").unwrap();
        let mut output = [0; 256];
        let mut workspace = Box::new(InflateWorkspace::new());
        let length = archive
            .read_entry(entry, &mut output, &mut workspace)
            .unwrap();
        let chapter = core::str::from_utf8(&output[..length]).unwrap();
        assert!(chapter.contains("Readable text."));
    }

    #[test]
    fn in_place_inflater_initialization_decodes_deflate() {
        let encoded = include_bytes!("../web/tests/fixtures/minimal.epub");
        let mut scratch = Box::new(ZipValidationScratch::new());
        let archive = StreamingZip::open(SliceFile(encoded), &mut scratch).unwrap();
        let entry = archive.find("EPUB/chapter.xhtml").unwrap();
        let mut workspace = Box::<InflateWorkspace>::new_uninit();
        // SAFETY: initialize_in_place establishes a valid value in the allocation.
        unsafe { InflateWorkspace::initialize_in_place(workspace.as_mut_ptr()) };
        // SAFETY: the allocation was initialized immediately above.
        let mut workspace = unsafe { workspace.assume_init() };
        let mut output = [0; 256];

        let length = archive
            .read_entry(entry, &mut output, &mut workspace)
            .unwrap();

        assert!(
            core::str::from_utf8(&output[..length])
                .unwrap()
                .contains("Readable text.")
        );
    }

    #[test]
    fn rejects_unsafe_paths_before_resource_lookup() {
        for path in ["", "/absolute", "../escape", "dir/../escape", "dir//file"] {
            assert!(!safe_path(path));
        }
        assert!(safe_path("OPS/Text/chapter.xhtml"));
    }

    #[test]
    fn rejects_non_zip_input() {
        let mut scratch = Box::new(ZipValidationScratch::new());
        assert!(matches!(
            StreamingZip::open(SliceFile(b"not a zip"), &mut scratch),
            Err(ZipError::MissingCentralDirectory)
        ));
    }
}
