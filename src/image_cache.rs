use core::fmt::Write;

#[cfg(test)]
mod tests;

use crate::{
    bounded_xml::FixedString,
    image::{Dither, PackedImage, READER_DEPTH, RenderOptions, ScaleMode, Size},
    image_decoder::{self, ImageDecodeError, ImageFormat, JpegDecodeWorkspace, stream},
    scratch::Scratch,
    storage::{BookFile, ImageFile},
    zip_stream::{InflateWorkspace, ReadAt, StreamingZip, ZipEntry, ZipError},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageResource {
    pub path: FixedString<128>,
    pub size: Size,
    pub crc32: u32,
    pub bytes: u32,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ImageProbeError<E> {
    Read(E),
    Image(ImageDecodeError),
}

impl<E> From<ZipError<E>> for ImageProbeError<E> {
    fn from(error: ZipError<E>) -> Self {
        match error {
            ZipError::Read(error) | ZipError::Write(error) => Self::Read(error),
            _ => Self::Image(ImageDecodeError::InvalidImage),
        }
    }
}

pub enum ImageSource<'a> {
    Book { file: &'a BookFile, path: &'a str },
    File(&'a ImageFile),
}

pub const IMAGE_WORKSPACE_BYTES: usize = 96_000;
pub const CACHE_HEADER_BYTES: usize = 432;
const KEY_BYTES: usize = 404;
const RENDERER_VERSION: u16 = 1;

pub struct ImageWorkspace {
    pub(crate) storage: Scratch<IMAGE_WORKSPACE_BYTES>,
}

impl ImageWorkspace {
    pub const fn new() -> Self {
        Self {
            storage: Scratch::new(),
        }
    }

    pub fn bytes(&mut self) -> &mut [u8; IMAGE_WORKSPACE_BYTES] {
        self.storage.bytes()
    }

    pub fn inflate(&mut self) -> &mut InflateWorkspace {
        // SAFETY: the initializer establishes a valid Raw inflater in exclusive scratch.
        unsafe {
            self.storage
                .initialize(InflateWorkspace::initialize_in_place)
        }
    }

    pub fn probe<R: ReadAt>(
        &mut self,
        archive: &StreamingZip<R>,
        entry: ZipEntry,
    ) -> Result<Size, ImageProbeError<R::Error>> {
        if entry.uncompressed_size() > stream::MAX_IMAGE_FILE_BYTES {
            return Err(ImageProbeError::Image(ImageDecodeError::InvalidImage));
        }
        // SAFETY: the initializer establishes both fields before a reference is formed.
        let workspace = unsafe { self.storage.initialize(ProbeWorkspace::initialize_in_place) };
        let length = archive
            .read_entry_prefix(entry, &mut workspace.prefix, &mut workspace.inflate)
            .map_err(ImageProbeError::from)?;
        stream::dimensions(&ImageBytes(&workspace.prefix[..length])).map_err(ImageProbeError::Image)
    }

    pub fn probe_resource<R: ReadAt>(
        &mut self,
        archive: &StreamingZip<R>,
        path: &str,
    ) -> Result<ImageResource, ImageProbeError<R::Error>> {
        let entry = archive.find(path).map_err(ImageProbeError::from)?;
        Ok(ImageResource {
            path: FixedString::try_from_str(path)
                .map_err(|_| ImageProbeError::Image(ImageDecodeError::InvalidImage))?,
            size: self.probe(archive, entry)?,
            crc32: entry.crc32(),
            bytes: entry.uncompressed_size(),
        })
    }

    pub fn decode<R: ReadAt>(
        &mut self,
        reader: &R,
        spec: ImageSpec,
        output: &mut [u8],
    ) -> Result<Size, ImageDecodeError> {
        let mut target = PackedImage::new(spec.size(), READER_DEPTH, output)
            .map_err(|_| ImageDecodeError::InvalidImage)?;
        let options = RenderOptions {
            scale: spec.scale,
            dither: Dither::None,
        };
        let report = match stream::format(reader)? {
            ImageFormat::Jpeg => image_decoder::decode_jpeg_reader(
                stream::Input::new(reader),
                &mut target,
                options,
                JpegDecodeWorkspace::in_buffer(self.storage.bytes())
                    .ok_or(ImageDecodeError::InvalidImage)?,
            )?,
            ImageFormat::Png => {
                // SAFETY: the initializer establishes every workspace field before borrowing it.
                let workspace = unsafe {
                    self.storage
                        .initialize(stream::PngWorkspace::initialize_in_place)
                };
                stream::decode_png(reader, &mut target, options, workspace)?
            }
        };
        Ok(report.source)
    }
}

impl Default for ImageWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

struct ProbeWorkspace {
    inflate: InflateWorkspace,
    prefix: [u8; 32 * 1024],
}

impl ProbeWorkspace {
    unsafe fn initialize_in_place(storage: *mut Self) {
        // SAFETY: the caller supplies aligned exclusive storage for both fields.
        unsafe {
            core::ptr::addr_of_mut!((*storage).prefix).write_bytes(0, 1);
            InflateWorkspace::initialize_in_place(core::ptr::addr_of_mut!((*storage).inflate));
        }
    }
}

pub struct ImageBytes<'a>(pub &'a [u8]);

impl ReadAt for ImageBytes<'_> {
    type Error = core::convert::Infallible;
    fn len(&self) -> u32 {
        self.0.len() as u32
    }
    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        let offset = offset as usize;
        let count = output.len().min(self.0.len().saturating_sub(offset));
        if count != 0 {
            output[..count].copy_from_slice(&self.0[offset..offset + count]);
        }
        Ok(count)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageSpec {
    width: u16,
    height: u16,
    scale: ScaleMode,
}

impl ImageSpec {
    pub fn new(width: usize, height: usize, scale: ScaleMode) -> Option<Self> {
        if width == 0 || width > 480 || !width.is_multiple_of(8) || height == 0 || height > 800 {
            return None;
        }
        Some(Self {
            width: width as u16,
            height: height as u16,
            scale,
        })
    }

    pub fn size(self) -> Size {
        Size::new(usize::from(self.width), usize::from(self.height)).expect("validated image spec")
    }

    pub const fn byte_len(self) -> usize {
        self.width as usize * self.height as usize / 4
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CacheSlot(pub(crate) u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidCacheName;

impl TryFrom<embedded_sdmmc::ShortFileName> for CacheSlot {
    type Error = InvalidCacheName;

    fn try_from(name: embedded_sdmmc::ShortFileName) -> Result<Self, Self::Error> {
        let base = name.base_name();
        if name.extension() != b"IMG" || base.len() != 8 || !base.iter().all(u8::is_ascii_hexdigit)
        {
            return Err(InvalidCacheName);
        }
        let base = core::str::from_utf8(base).map_err(|_| InvalidCacheName)?;
        u32::from_str_radix(base, 16)
            .map(Self)
            .map_err(|_| InvalidCacheName)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageKey {
    bytes: [u8; KEY_BYTES],
    spec: ImageSpec,
}

impl ImageKey {
    pub fn book(book: &BookFile, entry: ZipEntry, spec: ImageSpec) -> Option<Self> {
        Self::new(
            book.name().as_str(),
            book.size(),
            entry.path().as_str(),
            entry.crc32(),
            entry.uncompressed_size(),
            spec,
        )
    }

    pub fn resource(book: &BookFile, resource: &ImageResource, spec: ImageSpec) -> Option<Self> {
        Self::new(
            book.name().as_str(),
            book.size(),
            resource.path.as_str(),
            resource.crc32,
            resource.bytes,
            spec,
        )
    }

    pub fn file(name: &str, size: u32, crc32: u32, spec: ImageSpec) -> Option<Self> {
        Self::new("/files", 0, name, crc32, size, spec)
    }

    fn new(
        book: &str,
        size: u32,
        path: &str,
        crc: u32,
        length: u32,
        spec: ImageSpec,
    ) -> Option<Self> {
        if book.len() > 256 || path.len() > 128 || book.contains('\0') || path.contains('\0') {
            return None;
        }
        let mut bytes = [0; KEY_BYTES];
        bytes[..book.len()].copy_from_slice(book.as_bytes());
        bytes[256..260].copy_from_slice(&size.to_le_bytes());
        bytes[260..260 + path.len()].copy_from_slice(path.as_bytes());
        bytes[388..392].copy_from_slice(&crc.to_le_bytes());
        bytes[392..396].copy_from_slice(&length.to_le_bytes());
        bytes[396..398].copy_from_slice(&spec.width.to_le_bytes());
        bytes[398..400].copy_from_slice(&spec.height.to_le_bytes());
        bytes[400] = match spec.scale {
            ScaleMode::Contain => 0,
            ScaleMode::Cover => 1,
        };
        bytes[402..404].copy_from_slice(&RENDERER_VERSION.to_le_bytes());
        Some(Self { bytes, spec })
    }

    pub const fn spec(&self) -> ImageSpec {
        self.spec
    }

    pub fn slot(&self) -> CacheSlot {
        CacheSlot(crc32fast::hash(&self.bytes))
    }

    pub fn file_name(&self) -> FixedString<12> {
        let mut name = FixedString::new();
        write!(name, "{:08X}.IMG", crc32fast::hash(&self.bytes)).expect("fixed cache name");
        name
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheState {
    Hit,
    Prepared,
}

#[derive(Clone, Debug)]
pub struct PreparedImage {
    pub key: ImageKey,
    pub source: Size,
    pub state: CacheState,
}

impl PreparedImage {
    pub fn encode(&self, pixels: &[u8]) -> [u8; CACHE_HEADER_BYTES] {
        let mut bytes = [0; CACHE_HEADER_BYTES];
        bytes[..8].copy_from_slice(b"BRWIMG01");
        bytes[8..412].copy_from_slice(&self.key.bytes);
        bytes[412..414].copy_from_slice(&(self.source.width() as u16).to_le_bytes());
        bytes[414..416].copy_from_slice(&(self.source.height() as u16).to_le_bytes());
        bytes[416..420].copy_from_slice(&(pixels.len() as u32).to_le_bytes());
        bytes[420..424].copy_from_slice(&crc32fast::hash(pixels).to_le_bytes());
        let checksum = crc32fast::hash(&bytes[..424]);
        bytes[424..428].copy_from_slice(&checksum.to_le_bytes());
        bytes[428..].copy_from_slice(b"DONE");
        bytes
    }

    pub fn decode(bytes: &[u8; CACHE_HEADER_BYTES], key: &ImageKey, pixels: &[u8]) -> Option<Self> {
        let spec = key.spec();
        if bytes[..8] != *b"BRWIMG01"
            || bytes[8..412] != key.bytes
            || bytes[428..] != *b"DONE"
            || crc32fast::hash(&bytes[..424])
                != u32::from_le_bytes(bytes[424..428].try_into().ok()?)
            || pixels.len() != spec.byte_len()
            || u32::from_le_bytes(bytes[416..420].try_into().ok()?) as usize != pixels.len()
            || crc32fast::hash(pixels) != u32::from_le_bytes(bytes[420..424].try_into().ok()?)
        {
            return None;
        }
        let width = usize::from(u16::from_le_bytes(bytes[412..414].try_into().ok()?));
        let height = usize::from(u16::from_le_bytes(bytes[414..416].try_into().ok()?));
        let source = super::image_decoder::checked_size(width, height).ok()?;
        Some(Self {
            key: key.clone(),
            source,
            state: CacheState::Hit,
        })
    }
}
