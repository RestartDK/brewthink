use std::{boxed::Box, format, string::String, vec, vec::Vec};

use crate::{
    app::ReaderPreferences,
    bounded_layout::{BoundedPage, LayoutError, layout_xhtml_page},
    bounded_xml::FixedString,
    cover::{self, COVER_BYTES, CoverDecodeWorkspace, JpegDecodeWorkspace, encoded_cover_fits},
    device_epub::{
        DeviceEpub, DeviceEpubError, DevicePackageScratch, DevicePublication,
        MAX_DEVICE_RESOURCE_BYTES, MAX_DEVICE_SPINE_ITEMS,
    },
    image::{
        Dither, PackedBitmap, PackedImage, READER_DEPTH, RenderOptions, RgbImage, ScaleMode, Size,
    },
    image_decoder::{ImageDecodeError, ImageFormat, decode_jpeg, decode_png},
    navigation::CHAPTER_TITLE_BYTES,
    reader::{FRAME_HEIGHT, FRAME_WIDTH},
    storage::MAX_DEVICE_IMAGE_BYTES,
    zip_stream::{InflateWorkspace, ReadAt, StreamingZip, ZipError, ZipValidationScratch},
};
use core::convert::Infallible;

pub const FRAME_BYTES: usize = FRAME_WIDTH * FRAME_HEIGHT / 8 * READER_DEPTH.bits();

#[derive(Debug)]
pub enum SimulatorError {
    ArchiveTooLarge,
    Epub(DeviceEpubError<Infallible>),
    Zip(ZipError<Infallible>),
    Layout(LayoutError),
    Image(ImageDecodeError),
}

impl core::fmt::Display for SimulatorError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ArchiveTooLarge => f.write_str("EPUB exceeds the device archive address range"),
            Self::Epub(error) => write!(f, "EPUB: {error:?}"),
            Self::Zip(error) => write!(f, "ZIP: {error:?}"),
            Self::Layout(error) => write!(f, "chapter: {error}"),
            Self::Image(error) => write!(f, "cover: {error:?}"),
        }
    }
}

impl core::error::Error for SimulatorError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Layout(error) => Some(error),
            Self::ArchiveTooLarge | Self::Epub(_) | Self::Zip(_) | Self::Image(_) => None,
        }
    }
}

#[derive(Clone, Copy)]
struct MemoryFile<'a> {
    bytes: &'a [u8],
    length: u32,
}

impl<'a> TryFrom<&'a [u8]> for MemoryFile<'a> {
    type Error = SimulatorError;

    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        Ok(Self {
            bytes,
            length: u32::try_from(bytes.len()).map_err(|_| SimulatorError::ArchiveTooLarge)?,
        })
    }
}

impl ReadAt for MemoryFile<'_> {
    type Error = Infallible;

    fn len(&self) -> u32 {
        self.length
    }

    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        let Some(remaining) = self.bytes.get(offset as usize..) else {
            return Ok(0);
        };
        let length = output.len().min(remaining.len());
        output[..length].copy_from_slice(&remaining[..length]);
        Ok(length)
    }
}

pub struct Book {
    pub file_name: String,
    pub file_size: u32,
    pub title: String,
    pub creator: String,
    pub cover: Cover,
    pub chapters: Vec<Chapter>,
    pub navigation_error: Option<DeviceEpubError<Infallible>>,
}

impl Book {
    pub fn from_epub(encoded: &[u8], file_name: &str) -> Result<Self, SimulatorError> {
        let reader = MemoryFile::try_from(encoded)?;
        let mut zip = Box::new(ZipValidationScratch::new());
        let mut package = Box::new(DevicePackageScratch::new());
        let mut inflater = Box::new(InflateWorkspace::new());
        let mut resource = Box::new([0; MAX_DEVICE_RESOURCE_BYTES]);
        let mut publication = Box::new(DevicePublication::new());
        let epub = DeviceEpub::open(
            reader,
            &mut zip,
            &mut package,
            &mut inflater,
            &mut resource,
            &mut publication,
        )
        .map_err(SimulatorError::Epub)?;
        let publication = epub.publication();
        let mut titles =
            Box::new([FixedString::<CHAPTER_TITLE_BYTES>::new(); MAX_DEVICE_SPINE_ITEMS]);
        let navigation_error = epub
            .read_chapter_titles(&mut titles, &mut resource[..], &mut inflater)
            .err();
        let mut chapters = Vec::with_capacity(publication.spine_len());
        for index in 0..publication.spine_len() {
            let length = epub
                .read_spine(index, &mut resource[..], &mut inflater)
                .map_err(SimulatorError::Epub)?;
            let title = match titles[index].as_str() {
                "" => format!("Chapter {}", index + 1),
                title => title.into(),
            };
            chapters.push(Chapter::from_xhtml(&resource[..length], title)?);
        }
        let cover = Cover::read(
            reader,
            publication.cover_path(),
            &mut zip,
            &mut inflater,
            &mut resource,
        );
        Ok(Self {
            file_name: file_name.into(),
            file_size: reader.length,
            title: publication.title().into(),
            creator: publication.creator().into(),
            cover,
            chapters,
            navigation_error,
        })
    }
}

pub struct Chapter {
    title: String,
    xhtml: Box<[u8]>,
}

impl Chapter {
    fn from_xhtml(xhtml: &[u8], title: String) -> Result<Self, SimulatorError> {
        if xhtml.len() > MAX_DEVICE_RESOURCE_BYTES {
            return Err(SimulatorError::Epub(DeviceEpubError::ResourceTooLarge));
        }
        layout_xhtml_page(xhtml, 0, ReaderPreferences::default())
            .map_err(SimulatorError::Layout)?;
        Ok(Self {
            title,
            xhtml: xhtml.into(),
        })
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn page(
        &self,
        index: usize,
        preferences: ReaderPreferences,
    ) -> Result<BoundedPage, LayoutError> {
        layout_xhtml_page(&self.xhtml, index, preferences)
    }
}

pub enum Cover {
    Missing,
    TooLarge,
    Unsupported,
    Failed(SimulatorError),
    Decoded {
        shelf: Box<[u8; COVER_BYTES]>,
        original: OriginalFrame,
    },
}

pub enum OriginalFrame {
    TooLarge,
    Failed(ImageDecodeError),
    Decoded(Box<[u8; FRAME_BYTES]>),
}

impl Cover {
    pub fn bitmap(&self) -> Option<PackedBitmap<'_>> {
        match self {
            Self::Decoded { shelf, .. } => Some(cover::bitmap(shelf)),
            Self::Missing | Self::TooLarge | Self::Unsupported | Self::Failed(_) => None,
        }
    }

    pub fn frame_bitmap(&self) -> Option<PackedBitmap<'_>> {
        match self {
            Self::Decoded {
                original: OriginalFrame::Decoded(frame),
                ..
            } => Some(frame_bitmap(frame)),
            Self::Decoded {
                original: OriginalFrame::TooLarge | OriginalFrame::Failed(_),
                ..
            }
            | Self::Missing
            | Self::TooLarge
            | Self::Unsupported
            | Self::Failed(_) => None,
        }
    }

    fn read(
        reader: MemoryFile<'_>,
        path: Option<&str>,
        zip: &mut ZipValidationScratch,
        inflater: &mut InflateWorkspace,
        resource: &mut [u8; MAX_DEVICE_RESOURCE_BYTES],
    ) -> Self {
        let Some(path) = path else {
            return Self::Missing;
        };
        match Self::decode(reader, path, zip, inflater, resource) {
            Ok(cover) => cover,
            Err(error) => Self::Failed(error),
        }
    }

    fn decode(
        reader: MemoryFile<'_>,
        path: &str,
        zip: &mut ZipValidationScratch,
        inflater: &mut InflateWorkspace,
        resource: &mut [u8; MAX_DEVICE_RESOURCE_BYTES],
    ) -> Result<Self, SimulatorError> {
        let archive = StreamingZip::open(reader, zip).map_err(SimulatorError::Zip)?;
        let entry = archive.find(path).map_err(SimulatorError::Zip)?;
        if !encoded_cover_fits(entry.compressed_size(), entry.uncompressed_size()) {
            return Ok(Self::TooLarge);
        }
        let length = archive
            .read_entry(entry, resource, inflater)
            .map_err(SimulatorError::Zip)?;
        let encoded = &resource[..length];
        let Some(format) = ImageFormat::detect(encoded) else {
            return Ok(Self::Unsupported);
        };
        let mut shelf = Box::new([0xff; COVER_BYTES]);
        match format {
            ImageFormat::Png => {
                cover::decode_png_cover(encoded, &mut shelf, &mut CoverDecodeWorkspace::new())
            }
            ImageFormat::Jpeg => {
                cover::decode_jpeg_cover(encoded, &mut shelf, &mut JpegDecodeWorkspace::new())
            }
        }
        .map_err(SimulatorError::Image)?;
        let original = if entry.uncompressed_size() as usize > MAX_DEVICE_IMAGE_BYTES {
            OriginalFrame::TooLarge
        } else {
            OriginalFrame::decode(encoded, format)
        };
        Ok(Self::Decoded { shelf, original })
    }
}

impl OriginalFrame {
    fn decode(encoded: &[u8], format: ImageFormat) -> Self {
        let mut pixels = Box::new([0xff; FRAME_BYTES]);
        let mut target = PackedImage::new(frame_size(), READER_DEPTH, &mut pixels[..])
            .expect("the packed frame buffer has the exact required length");
        let options = RenderOptions {
            scale: ScaleMode::Contain,
            dither: Dither::None,
        };
        let decoded = match format {
            ImageFormat::Png => decode_png(
                encoded,
                &mut target,
                options,
                &mut CoverDecodeWorkspace::new(),
            ),
            ImageFormat::Jpeg => decode_jpeg(
                encoded,
                &mut target,
                options,
                &mut JpegDecodeWorkspace::new(),
            ),
        };
        match decoded {
            Ok(_) => Self::Decoded(pixels),
            Err(error) => Self::Failed(error),
        }
    }
}

fn frame_size() -> Size {
    Size::new(FRAME_WIDTH, FRAME_HEIGHT).expect("frame dimensions are non-zero")
}

fn frame_bitmap(bytes: &[u8; FRAME_BYTES]) -> PackedBitmap<'_> {
    PackedBitmap::new(frame_size(), READER_DEPTH, bytes)
        .expect("the packed frame buffer has the exact required length")
}

pub fn sample_books() -> Result<Vec<Book>, SimulatorError> {
    let samples = [
        (
            "study-in-scarlet.epub",
            "A Study in Scarlet",
            "Arthur Conan Doyle",
        ),
        (
            "pride-and-prejudice.epub",
            "Pride and Prejudice",
            "Jane Austen",
        ),
        ("walden.epub", "Walden", "Henry David Thoreau"),
        ("frankenstein.epub", "Frankenstein", "Mary Shelley"),
    ];
    let mut books = Vec::with_capacity(samples.len());
    for (index, (file_name, title, creator)) in samples.into_iter().enumerate() {
        let mut chapters = Vec::with_capacity(3);
        for chapter in 0..3 {
            let mut xhtml = String::from("<html><body>");
            for paragraph in 0..18 {
                let tag = if paragraph == 0 { "h1" } else { "p" };
                xhtml.push_str(&format!("<{tag}>{title} · section {} · passage {}. This public-domain sample proves page turning, chapter boundaries, sleep, wake, and reading-position resume in the shared application state.</{tag}>", chapter + 1, paragraph + 1));
            }
            xhtml.push_str("</body></html>");
            chapters.push(Chapter::from_xhtml(
                xhtml.as_bytes(),
                format!("Section {}", chapter + 1),
            )?);
        }
        books.push(Book {
            file_name: file_name.into(),
            file_size: 180_000 + index as u32 * 74_000,
            title: title.into(),
            creator: creator.into(),
            cover: sample_cover(index),
            chapters,
            navigation_error: None,
        });
    }
    Ok(books)
}

fn sample_cover(index: usize) -> Cover {
    const WIDTH: usize = 48;
    const HEIGHT: usize = 72;
    let mut rgb = vec![255; WIDTH * HEIGHT * 3];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let border = x < 2 || y < 2 || x >= WIDTH - 2 || y >= HEIGHT - 2;
            let pattern = match index {
                0 => (x / 6 + y / 6) % 2 == 0,
                1 => x % 11 < 3,
                2 => (x + y) % 13 < 4,
                _ => x.abs_diff(WIDTH / 2) + y.abs_diff(HEIGHT / 2) < 18,
            };
            let offset = (y * WIDTH + x) * 3;
            rgb[offset..offset + 3].fill(if border || pattern { 24 } else { 232 });
        }
    }
    let source = RgbImage::new(Size::new(WIDTH, HEIGHT).unwrap(), &rgb).unwrap();
    let mut shelf = Box::new([0xff; COVER_BYTES]);
    render_sample(
        &source,
        Size::new(cover::COVER_WIDTH, cover::COVER_HEIGHT).unwrap(),
        &mut shelf[..],
        ScaleMode::Cover,
    );
    let mut frame = Box::new([0xff; FRAME_BYTES]);
    render_sample(&source, frame_size(), &mut frame[..], ScaleMode::Contain);
    Cover::Decoded {
        shelf,
        original: OriginalFrame::Decoded(frame),
    }
}

fn render_sample(source: &RgbImage<'_>, size: Size, output: &mut [u8], scale: ScaleMode) {
    let mut target = PackedImage::new(size, READER_DEPTH, output).unwrap();
    crate::image::render(
        source,
        &mut target,
        RenderOptions {
            scale,
            dither: Dither::None,
        },
    );
}

#[cfg(test)]
mod tests;
