use embedded_graphics::pixelcolor::{Rgb888, RgbColor};
use embedded_png::{DontDraw, ParsedPng};
use tjpgd_rs::{JpegDecoder, PixelFormat, Rect, Scale};

use crate::image::{Dither, PackedImage, RenderOptions, ScaleMode, Size};

#[cfg(feature = "device-reader")]
pub mod stream;

pub const MAX_IMAGE_DIMENSION: usize = 4_096;
pub const MAX_DECODED_IMAGE_PIXELS: usize = 8 * 1_024 * 1_024;
const DEFLATE_WINDOW_BYTES: usize = 32 * 1024;
const MAX_SCANLINE_BYTES: usize = MAX_IMAGE_DIMENSION * 4;
const JPEG_WORKSPACE_BYTES: usize = 35_000;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum ImageFormat {
    Jpeg = 1,
    Png = 2,
}

impl ImageFormat {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Png => "png",
        }
    }

    pub fn detect(encoded: &[u8]) -> Option<Self> {
        if encoded.starts_with(&[0xFF, 0xD8]) {
            Some(Self::Jpeg)
        } else if encoded.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::Png)
        } else {
            None
        }
    }

    pub const fn from_name(name: &str) -> Option<Self> {
        match name.as_bytes() {
            b"jpeg" | b"jpg" => Some(Self::Jpeg),
            b"png" => Some(Self::Png),
            _ => None,
        }
    }
}

pub struct PngDecodeWorkspace {
    deflate: [u8; DEFLATE_WINDOW_BYTES],
    scanline: [u8; MAX_SCANLINE_BYTES],
}

impl PngDecodeWorkspace {
    pub const fn new() -> Self {
        Self {
            deflate: [0; DEFLATE_WINDOW_BYTES],
            scanline: [0; MAX_SCANLINE_BYTES],
        }
    }

    pub fn in_buffer(bytes: &mut [u8]) -> Option<&mut Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let pointer = bytes.as_mut_ptr().cast::<Self>();
        // SAFETY: this concrete workspace contains only byte arrays, has byte
        // alignment, fits in the exclusively borrowed initialized byte slice,
        // and does not outlive that slice.
        Some(unsafe { &mut *pointer })
    }
}

impl Default for PngDecodeWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

pub struct JpegDecodeWorkspace {
    bytes: [u8; JPEG_WORKSPACE_BYTES],
}

impl JpegDecodeWorkspace {
    pub const fn new() -> Self {
        Self {
            bytes: [0; JPEG_WORKSPACE_BYTES],
        }
    }

    pub fn in_buffer(bytes: &mut [u8]) -> Option<&mut Self> {
        if bytes.len() < core::mem::size_of::<Self>() {
            return None;
        }
        let pointer = bytes.as_mut_ptr().cast::<Self>();
        // SAFETY: this concrete workspace contains only a byte array, has byte
        // alignment, fits in the exclusively borrowed initialized byte slice,
        // and does not outlive that slice.
        Some(unsafe { &mut *pointer })
    }
}

impl Default for JpegDecodeWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodeReport {
    pub format: ImageFormat,
    pub source: Size,
    pub target: Size,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageDecodeError {
    InvalidImage,
    FormatMismatch,
    DimensionsOutOfRange,
}

pub fn decode_png(
    encoded: &[u8],
    target: &mut PackedImage<'_>,
    options: RenderOptions,
    workspace: &mut PngDecodeWorkspace,
) -> Result<DecodeReport, ImageDecodeError> {
    if ImageFormat::detect(encoded) != Some(ImageFormat::Png) {
        return Err(ImageDecodeError::FormatMismatch);
    }
    let png = ParsedPng::from_bytes(encoded, true, DontDraw)
        .map_err(|_| ImageDecodeError::InvalidImage)?;
    let source = checked_size(png.header.width, png.header.height)?;
    let scanline_bytes = png_scanline_bytes(encoded, source.width())?;
    if scanline_bytes > workspace.scanline.len() {
        return Err(ImageDecodeError::DimensionsOutOfRange);
    }
    let transform = Transform::new(source, target.size(), options.scale);
    target.clear_white();
    png.draw_to_fn::<_, Rgb888>(
        &mut workspace.deflate,
        &mut workspace.scanline[..scanline_bytes],
        |x, y, (alpha, color)| {
            let foreground = (u32::from(color.r()) * 54
                + u32::from(color.g()) * 183
                + u32::from(color.b()) * 19
                + 128)
                >> 8;
            let alpha = u32::from(alpha);
            let luma = ((foreground * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
            transform.draw_source_pixel(target, x, y, luma, options.dither);
            Ok(())
        },
    )
    .map_err(|_| ImageDecodeError::InvalidImage)?;
    Ok(DecodeReport {
        format: ImageFormat::Png,
        source,
        target: target.size(),
    })
}

pub fn decode_jpeg(
    encoded: &[u8],
    target: &mut PackedImage<'_>,
    options: RenderOptions,
    workspace: &mut JpegDecodeWorkspace,
) -> Result<DecodeReport, ImageDecodeError> {
    if ImageFormat::detect(encoded) != Some(ImageFormat::Jpeg) {
        return Err(ImageDecodeError::FormatMismatch);
    }
    decode_jpeg_reader(
        SliceReader { remaining: encoded },
        target,
        options,
        workspace,
    )
}

pub fn decode_jpeg_reader<R: embedded_io::Read>(
    reader: R,
    target: &mut PackedImage<'_>,
    options: RenderOptions,
    workspace: &mut JpegDecodeWorkspace,
) -> Result<DecodeReport, ImageDecodeError> {
    let mut decoder = JpegDecoder::new(&mut workspace.bytes[..], reader)
        .map_err(|_| ImageDecodeError::InvalidImage)?;
    let original = checked_size(usize::from(decoder.width()), usize::from(decoder.height()))?;
    let scale = jpeg_scale(original, target.size(), options.scale);
    let source = checked_size(
        original.width() >> scale.shift(),
        original.height() >> scale.shift(),
    )?;
    let transform = Transform::new(source, target.size(), options.scale);
    target.clear_white();
    decoder
        .decode(
            scale,
            PixelFormat::Grayscale,
            &mut |pixels: &[u8], rect: &Rect| {
                let mut source_offset = 0usize;
                for y in usize::from(rect.top)..=usize::from(rect.bottom) {
                    for x in usize::from(rect.left)..=usize::from(rect.right) {
                        let luma = pixels.get(source_offset).copied().unwrap_or(0xFF);
                        source_offset += 1;
                        transform.draw_source_pixel(target, x, y, luma, options.dither);
                    }
                }
                true
            },
        )
        .map_err(|_| ImageDecodeError::InvalidImage)?;
    Ok(DecodeReport {
        format: ImageFormat::Jpeg,
        source: original,
        target: target.size(),
    })
}

pub fn decode(
    format: ImageFormat,
    encoded: &[u8],
    target: &mut PackedImage<'_>,
    options: RenderOptions,
    png: &mut PngDecodeWorkspace,
    jpeg: &mut JpegDecodeWorkspace,
) -> Result<DecodeReport, ImageDecodeError> {
    match format {
        ImageFormat::Jpeg => decode_jpeg(encoded, target, options, jpeg),
        ImageFormat::Png => decode_png(encoded, target, options, png),
    }
}

struct SliceReader<'a> {
    remaining: &'a [u8],
}

impl embedded_io::ErrorType for SliceReader<'_> {
    type Error = core::convert::Infallible;
}

impl embedded_io::Read for SliceReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> Result<usize, Self::Error> {
        let length = output.len().min(self.remaining.len());
        output[..length].copy_from_slice(&self.remaining[..length]);
        self.remaining = &self.remaining[length..];
        Ok(length)
    }
}

#[derive(Clone, Copy)]
struct Transform {
    x: AxisTransform,
    y: AxisTransform,
}

#[derive(Clone, Copy)]
enum AxisOffset {
    Inset(usize),
    Crop(usize),
}

#[derive(Clone, Copy)]
struct AxisTransform {
    source: usize,
    target: usize,
    quotient: usize,
    remainder: usize,
    offset: AxisOffset,
}

impl AxisTransform {
    fn new(source: usize, scaled: usize, target: usize) -> Self {
        debug_assert!((1..=MAX_IMAGE_DIMENSION).contains(&source));
        Self {
            source,
            target,
            quotient: scaled / source,
            remainder: scaled % source,
            offset: if scaled <= target {
                AxisOffset::Inset((target - scaled) / 2)
            } else {
                AxisOffset::Crop((scaled - target) / 2)
            },
        }
    }

    fn edge(self, position: usize) -> usize {
        let scaled = position * self.quotient + position * self.remainder / self.source;
        match self.offset {
            AxisOffset::Inset(offset) => scaled + offset,
            AxisOffset::Crop(offset) => scaled.saturating_sub(offset),
        }
        .min(self.target)
    }

    fn range(self, position: usize) -> core::ops::Range<usize> {
        if position >= self.source {
            return 0..0;
        }
        self.edge(position)..self.edge(position + 1)
    }
}

impl Transform {
    fn new(source: Size, target: Size, mode: ScaleMode) -> Self {
        let scaled = scaled_size(source, target, mode);
        Self {
            x: AxisTransform::new(source.width(), scaled.width(), target.width()),
            y: AxisTransform::new(source.height(), scaled.height(), target.height()),
        }
    }

    fn draw_source_pixel(
        self,
        target: &mut PackedImage<'_>,
        source_x: usize,
        source_y: usize,
        luma: u8,
        dither: Dither,
    ) {
        let rows = self.y.range(source_y);
        if rows.is_empty() {
            return;
        }
        let columns = self.x.range(source_x);
        for y in rows {
            for x in columns.clone() {
                target.set_luma_dithered(x, y, luma, dither);
            }
        }
    }
}

pub(crate) fn checked_size(width: usize, height: usize) -> Result<Size, ImageDecodeError> {
    if width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || width
            .checked_mul(height)
            .is_none_or(|pixels| pixels > MAX_DECODED_IMAGE_PIXELS)
    {
        return Err(ImageDecodeError::DimensionsOutOfRange);
    }
    Size::new(width, height).map_err(|_| ImageDecodeError::DimensionsOutOfRange)
}

fn jpeg_scale(source: Size, target: Size, mode: ScaleMode) -> Scale {
    for scale in [Scale::Eighth, Scale::Quarter, Scale::Half] {
        let Ok(candidate) = checked_size(
            source.width() >> scale.shift(),
            source.height() >> scale.shift(),
        ) else {
            continue;
        };
        let transformed = scaled_size(candidate, target, mode);
        if candidate.width() >= transformed.width() && candidate.height() >= transformed.height() {
            return scale;
        }
    }
    Scale::None
}

fn png_scanline_bytes(encoded: &[u8], width: usize) -> Result<usize, ImageDecodeError> {
    if encoded.len() < 29 || encoded[28] != 0 {
        return Err(ImageDecodeError::InvalidImage);
    }
    let channels = match encoded[25] {
        0 | 3 => 1usize,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err(ImageDecodeError::InvalidImage),
    };
    let bytes_per_pixel = (usize::from(encoded[24]) * channels).div_ceil(8);
    width
        .checked_mul(bytes_per_pixel)
        .ok_or(ImageDecodeError::DimensionsOutOfRange)
}

fn scaled_size(source: Size, target: Size, mode: ScaleMode) -> Size {
    let width_limited = (target.width() as u128) * (source.height() as u128)
        <= (target.height() as u128) * (source.width() as u128);
    let scale_to_width = match mode {
        ScaleMode::Contain => width_limited,
        ScaleMode::Cover => !width_limited,
    };
    if scale_to_width {
        Size::new(
            target.width(),
            rounded_ratio(source.height(), target.width(), source.width()),
        )
        .expect("scaled image dimensions are non-zero")
    } else {
        Size::new(
            rounded_ratio(source.width(), target.height(), source.height()),
            target.height(),
        )
        .expect("scaled image dimensions are non-zero")
    }
}

fn rounded_ratio(value: usize, numerator: usize, denominator: usize) -> usize {
    let result =
        ((value as u128) * (numerator as u128) + denominator as u128 / 2) / denominator as u128;
    usize::try_from(result).unwrap_or(usize::MAX).max(1)
}

#[cfg(test)]
mod cover_tests;
#[cfg(test)]
mod scaling_tests;
#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec;

    use super::{
        ImageDecodeError, ImageFormat, JpegDecodeWorkspace, PngDecodeWorkspace, decode_jpeg,
        decode_png,
    };
    use crate::image::{PackedImage, RenderOptions, ScaleMode, Size};

    const TRANSPARENT_PNG: &[u8] = include_bytes!("../web/tests/fixtures/transparent.png");

    #[test]
    fn png_and_jpeg_preserve_four_tones_without_dither() {
        use crate::image::{Dither, PixelDepth};
        let depth = PixelDepth::Four;

        let size = Size::new(64, 16).unwrap();
        let mut bytes = vec![0; depth.byte_len(size).unwrap()];
        let mut image = PackedImage::new(size, depth, &mut bytes).unwrap();
        let options = RenderOptions {
            scale: ScaleMode::Contain,
            dither: Dither::None,
        };
        for format in [ImageFormat::Png, ImageFormat::Jpeg] {
            match format {
                ImageFormat::Png => decode_png(
                    include_bytes!("../web/tests/fixtures/gray-ramp.png"),
                    &mut image,
                    options,
                    &mut PngDecodeWorkspace::new(),
                ),
                ImageFormat::Jpeg => decode_jpeg(
                    include_bytes!("../web/tests/fixtures/gray-ramp.jpg"),
                    &mut image,
                    options,
                    &mut JpegDecodeWorkspace::new(),
                ),
            }
            .unwrap();
            let mut seen = 0u8;
            for band in 0..8 {
                let level = image.bitmap().level(band * 8 + 4, 8);
                seen |= 1 << level;
                for y in 2..14 {
                    for x in band * 8 + 2..band * 8 + 6 {
                        assert_eq!(image.bitmap().level(x, y), level, "{depth:?} {format:?}");
                    }
                }
            }
            assert_eq!(
                seen,
                ((1u16 << depth.levels()) - 1) as u8,
                "{depth:?} {format:?}"
            );
            assert_eq!(image.bitmap().level(4, 8), 0);
            assert_eq!(image.bitmap().level(60, 8), depth.levels() - 1);
        }
    }

    #[test]
    fn detects_supported_formats() {
        assert_eq!(
            ImageFormat::detect(b"\xff\xd8rest"),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            ImageFormat::detect(b"\x89PNG\r\n\x1a\nrest"),
            Some(ImageFormat::Png)
        );
        assert_eq!(ImageFormat::detect(b"GIF89a"), None);
    }

    #[test]
    fn decodes_jpeg_directly_into_a_full_frame() {
        let encoded = include_bytes!("../web/tests/fixtures/cover.jpg");
        let mut bytes = vec![0xFF; 480 * 800 / 8];
        let mut target = PackedImage::monochrome(Size::new(480, 800).unwrap(), &mut bytes).unwrap();
        decode_jpeg(
            encoded,
            &mut target,
            RenderOptions {
                scale: ScaleMode::Cover,
                ..RenderOptions::default()
            },
            &mut JpegDecodeWorkspace::new(),
        )
        .unwrap();
        assert!(bytes.iter().any(|byte| *byte != 0xFF));
        assert!(bytes.iter().any(|byte| *byte != 0x00));
    }

    #[test]
    fn jpeg_rejects_dimensions_above_the_decode_budget() {
        let mut encoded = include_bytes!("../web/tests/fixtures/cover.jpg").to_vec();
        let start = encoded
            .windows(2)
            .position(|marker| marker == [0xFF, 0xC0])
            .unwrap();
        encoded[start + 5..start + 7].copy_from_slice(&u16::MAX.to_be_bytes());
        encoded[start + 7..start + 9].copy_from_slice(&u16::MAX.to_be_bytes());
        let mut bytes = vec![0xFF; 64 * 16 / 8];
        let mut target = PackedImage::monochrome(Size::new(64, 16).unwrap(), &mut bytes).unwrap();
        assert_eq!(
            decode_jpeg(
                &encoded,
                &mut target,
                RenderOptions {
                    scale: ScaleMode::Contain,
                    ..RenderOptions::default()
                },
                &mut JpegDecodeWorkspace::new(),
            ),
            Err(ImageDecodeError::DimensionsOutOfRange)
        );
    }

    #[test]
    fn decodes_png_into_multiple_target_shapes() {
        let mut workspace = PngDecodeWorkspace::new();
        for (width, height) in [(176, 264), (480, 800)] {
            let mut bytes = vec![0xFF; width * height / 8];
            let mut target =
                PackedImage::monochrome(Size::new(width, height).unwrap(), &mut bytes).unwrap();
            decode_png(
                TRANSPARENT_PNG,
                &mut target,
                RenderOptions {
                    scale: ScaleMode::Contain,
                    ..RenderOptions::default()
                },
                &mut workspace,
            )
            .unwrap();
            assert!(bytes.iter().all(|byte| *byte == 0xFF));
        }
    }
}
