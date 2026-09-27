use core::fmt::Write;

#[cfg(test)]
mod tests;

use crate::{
    app::ReaderPreferences,
    bounded_layout::{ChapterSummary, MAX_CHAPTER_TITLE_BYTES, MAX_ENCODED_PAGE_BYTES},
    bounded_xml::{FixedString, stream::XmlWorkspace},
    device_epub::MAX_DEVICE_RESOURCE_BYTES,
    image_cache::CacheState,
    scratch::Scratch,
    storage::{BookFile, BookFileName},
    zip_stream::{ReadAt, ZipEntry},
};

pub const MAX_CHAPTER_BYTES: u32 = 16 * 1024 * 1024;
pub const MAX_CHAPTER_PAGES: usize = 8192;
pub const SOURCE_HEADER_BYTES: usize = 416;
pub const CHAPTER_HEADER_BYTES: usize = 540;
pub const MAX_CHAPTER_CACHE_BYTES: u32 = 32 * 1024 * 1024;
const SOURCE_KEY_BYTES: usize = 400;
const LAYOUT_VERSION: u32 = 1;
pub(crate) const INDEX_BYTES: usize = (MAX_CHAPTER_PAGES + 1) * 4;

pub struct ChapterWorkspace {
    storage: Scratch<MAX_DEVICE_RESOURCE_BYTES>,
}

impl ChapterWorkspace {
    pub const fn new() -> Self {
        Self {
            storage: Scratch::new(),
        }
    }
    pub fn bytes(&mut self) -> &mut [u8; MAX_DEVICE_RESOURCE_BYTES] {
        self.storage.bytes()
    }
    pub(crate) fn layout(&mut self) -> &mut LayoutWorkspace {
        // SAFETY: the initializer establishes every field before exposing the typed scratch.
        unsafe {
            self.storage
                .initialize(LayoutWorkspace::initialize_in_place)
        }
    }
}

impl Default for ChapterWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct LayoutWorkspace {
    pub xml: XmlWorkspace,
    pub index: [u8; INDEX_BYTES],
    pub record: [u8; MAX_ENCODED_PAGE_BYTES],
    pub buffer: [u8; 4096],
}

impl LayoutWorkspace {
    unsafe fn initialize_in_place(storage: *mut Self) {
        // SAFETY: each field is initialized in the caller's aligned exclusive allocation.
        unsafe {
            XmlWorkspace::initialize_in_place(core::ptr::addr_of_mut!((*storage).xml));
            core::ptr::addr_of_mut!((*storage).index).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*storage).record).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*storage).buffer).write_bytes(0, 1);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChapterSourceKey {
    bytes: [u8; SOURCE_KEY_BYTES],
}

impl ChapterSourceKey {
    pub fn new(book: &BookFile, entry: ZipEntry, directory_crc: u32) -> Option<Self> {
        let path = entry.path();
        let path = path.as_str();
        let name = book.name().as_str();
        if name.len() > 256
            || name.contains('\0')
            || path.len() > 128
            || entry.uncompressed_size() > MAX_CHAPTER_BYTES
        {
            return None;
        }
        let mut bytes = [0; SOURCE_KEY_BYTES];
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        bytes[256..260].copy_from_slice(&book.size().to_le_bytes());
        bytes[260..260 + path.len()].copy_from_slice(path.as_bytes());
        bytes[388..392].copy_from_slice(&entry.crc32().to_le_bytes());
        bytes[392..396].copy_from_slice(&entry.uncompressed_size().to_le_bytes());
        bytes[396..400].copy_from_slice(&directory_crc.to_le_bytes());
        Some(Self { bytes })
    }
    pub fn book(&self) -> BookFile {
        let name = Self::text(&self.bytes[..256]);
        BookFile::new(
            BookFileName::try_from(name).expect("source key contains a valid book name"),
            u32::from_le_bytes(self.bytes[256..260].try_into().unwrap()),
        )
    }
    pub fn path(&self) -> &str {
        Self::text(&self.bytes[260..388])
    }
    fn text(bytes: &[u8]) -> &str {
        let length = bytes
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(bytes.len());
        core::str::from_utf8(&bytes[..length]).expect("source key contains validated UTF-8")
    }
    pub fn length(&self) -> u32 {
        u32::from_le_bytes(self.bytes[392..396].try_into().unwrap())
    }
    pub fn checksum(&self) -> u32 {
        u32::from_le_bytes(self.bytes[388..392].try_into().unwrap())
    }
    pub fn file_name(&self) -> FixedString<12> {
        name(crc32fast::hash(&self.bytes), "HTM")
    }
    pub fn header(&self) -> [u8; SOURCE_HEADER_BYTES] {
        let mut bytes = [0; SOURCE_HEADER_BYTES];
        bytes[..8].copy_from_slice(b"BRWTXT01");
        bytes[8..408].copy_from_slice(&self.bytes);
        let crc = crc32fast::hash(&bytes[..408]);
        bytes[408..412].copy_from_slice(&crc.to_le_bytes());
        bytes[412..].copy_from_slice(b"DONE");
        bytes
    }
    pub fn matches_header(&self, bytes: &[u8; SOURCE_HEADER_BYTES], file_length: u32) -> bool {
        file_length == SOURCE_HEADER_BYTES as u32 + self.length() && *bytes == self.header()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChapterKey {
    source: ChapterSourceKey,
    preferences: ReaderPreferences,
}

impl ChapterKey {
    pub const fn new(source: ChapterSourceKey, preferences: ReaderPreferences) -> Self {
        Self {
            source,
            preferences,
        }
    }
    pub const fn source(&self) -> &ChapterSourceKey {
        &self.source
    }
    pub const fn preferences(&self) -> ReaderPreferences {
        self.preferences
    }
    fn bytes(&self) -> [u8; 408] {
        let mut bytes = [0; 408];
        bytes[..400].copy_from_slice(&self.source.bytes);
        bytes[400..404].copy_from_slice(&self.preferences.packed().to_le_bytes());
        bytes[404..408].copy_from_slice(&LAYOUT_VERSION.to_le_bytes());
        bytes
    }
    pub fn file_name(&self) -> FixedString<12> {
        name(crc32fast::hash(&self.bytes()), "PGS")
    }
}

fn name(hash: u32, extension: &str) -> FixedString<12> {
    let mut name = FixedString::new();
    write!(name, "{hash:08X}.{extension}").expect("fixed chapter cache name");
    name
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChapterHeader {
    pub summary: ChapterSummary,
    pub index_offset: u32,
    pub file_length: u32,
    pub index_crc: u32,
}

impl ChapterHeader {
    pub fn encode(self, key: &ChapterKey) -> [u8; CHAPTER_HEADER_BYTES] {
        let mut bytes = [0; CHAPTER_HEADER_BYTES];
        bytes[..8].copy_from_slice(b"BRWPAG01");
        bytes[8..416].copy_from_slice(&key.bytes());
        bytes[416..420].copy_from_slice(&(self.summary.page_count as u32).to_le_bytes());
        bytes[420..424].copy_from_slice(&self.index_offset.to_le_bytes());
        bytes[424..428].copy_from_slice(&self.file_length.to_le_bytes());
        bytes[428..432].copy_from_slice(&self.index_crc.to_le_bytes());
        let title = self.summary.title.as_str().as_bytes();
        bytes[432..434].copy_from_slice(&(title.len() as u16).to_le_bytes());
        bytes[434..434 + title.len()].copy_from_slice(title);
        let crc = crc32fast::hash(&bytes[..532]);
        bytes[532..536].copy_from_slice(&crc.to_le_bytes());
        bytes[536..].copy_from_slice(b"DONE");
        bytes
    }

    pub fn decode(
        bytes: &[u8; CHAPTER_HEADER_BYTES],
        key: &ChapterKey,
        file_length: u32,
    ) -> Option<Self> {
        if &bytes[..8] != b"BRWPAG01"
            || bytes[8..416] != key.bytes()
            || &bytes[536..] != b"DONE"
            || crc32fast::hash(&bytes[..532])
                != u32::from_le_bytes(bytes[532..536].try_into().ok()?)
            || bytes[530..532] != [0, 0]
        {
            return None;
        }
        let page_count = u32::from_le_bytes(bytes[416..420].try_into().ok()?) as usize;
        let index_offset = u32::from_le_bytes(bytes[420..424].try_into().ok()?);
        let stored_length = u32::from_le_bytes(bytes[424..428].try_into().ok()?);
        if page_count == 0
            || page_count > MAX_CHAPTER_PAGES
            || stored_length != file_length
            || file_length > MAX_CHAPTER_CACHE_BYTES
            || index_offset < CHAPTER_HEADER_BYTES as u32
            || index_offset.checked_add((page_count as u32 + 1) * 4)? != file_length
        {
            return None;
        }
        let title_length = usize::from(u16::from_le_bytes(bytes[432..434].try_into().ok()?));
        if title_length > MAX_CHAPTER_TITLE_BYTES
            || bytes[434 + title_length..530].iter().any(|&byte| byte != 0)
        {
            return None;
        }
        let title = core::str::from_utf8(&bytes[434..434 + title_length]).ok()?;
        if !title.chars().all(crate::bounded_xml::is_xml_character) {
            return None;
        }
        Some(Self {
            summary: ChapterSummary {
                page_count,
                title: FixedString::try_from_str(title).ok()?,
            },
            index_offset,
            file_length,
            index_crc: u32::from_le_bytes(bytes[428..432].try_into().ok()?),
        })
    }

    pub fn page_range(self, index: &[u8], page: usize) -> Option<core::ops::Range<u32>> {
        if page >= self.summary.page_count
            || index.len() != (self.summary.page_count + 1) * 4
            || crc32fast::hash(index) != self.index_crc
        {
            return None;
        }
        let mut previous = None;
        for bytes in index.chunks_exact(4) {
            let offset = u32::from_le_bytes(bytes.try_into().ok()?);
            if offset < CHAPTER_HEADER_BYTES as u32
                || offset > self.index_offset
                || previous.is_some_and(|old| {
                    offset <= old || offset - old > MAX_ENCODED_PAGE_BYTES as u32 + 8
                })
            {
                return None;
            }
            previous = Some(offset);
        }
        if previous != Some(self.index_offset)
            || u32::from_le_bytes(index[..4].try_into().ok()?) != CHAPTER_HEADER_BYTES as u32
        {
            return None;
        }
        let start = u32::from_le_bytes(index[page * 4..page * 4 + 4].try_into().ok()?);
        let end = u32::from_le_bytes(index[page * 4 + 4..page * 4 + 8].try_into().ok()?);
        if end - start < 9 {
            return None;
        }
        Some(start..end)
    }
}

pub struct ChapterRequest<'a> {
    pub book: &'a BookFile,
    pub path: &'a str,
    pub page_index: usize,
    pub preferences: ReaderPreferences,
}

pub struct PreparedChapter {
    pub key: ChapterKey,
    pub state: CacheState,
    pub summary: ChapterSummary,
}

pub struct ChapterText<'a, R> {
    source: &'a R,
    length: u32,
}

impl<'a, R: ReadAt> ChapterText<'a, R> {
    pub fn new(source: &'a R, key: &ChapterSourceKey) -> Option<Self> {
        (source.len() == SOURCE_HEADER_BYTES as u32 + key.length()).then_some(Self {
            source,
            length: key.length(),
        })
    }
}

impl<R: ReadAt> ReadAt for ChapterText<'_, R> {
    type Error = R::Error;
    fn len(&self) -> u32 {
        self.length
    }
    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        let length = output
            .len()
            .min(self.length.saturating_sub(offset) as usize);
        if length == 0 {
            return Ok(0);
        }
        self.source
            .read_at(SOURCE_HEADER_BYTES as u32 + offset, &mut output[..length])
    }
}
