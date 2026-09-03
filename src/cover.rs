use embedded_graphics::pixelcolor::{Rgb888, RgbColor};
use embedded_png::{DontDraw, ParsedPng};
use tjpgd_rs::{JpegDecoder, PixelFormat, Rect, Scale};

use crate::image::{MonochromeBitmap, Size};

pub const COVER_WIDTH: usize = 176;
pub const COVER_HEIGHT: usize = 264;
pub const COVER_BYTES: usize = COVER_WIDTH * COVER_HEIGHT / 8;
pub const MAX_COVER_DIMENSION: usize = 1_536;
pub const MAX_ENCODED_COVER_BYTES: u32 = 128 * 1024;
const MAX_DECODED_COVER_PIXELS: usize = 1_024 * 1_536;
const DEFLATE_WINDOW_BYTES: usize = 32 * 1024;
const MAX_SCANLINE_BYTES: usize = MAX_COVER_DIMENSION * 4;
const JPEG_WORKSPACE_BYTES: usize = 35_000;

pub struct CoverDecodeWorkspace {
    deflate: [u8; DEFLATE_WINDOW_BYTES],
    scanline: [u8; MAX_SCANLINE_BYTES],
}

impl CoverDecodeWorkspace {
    pub const fn new() -> Self {
        Self {
            deflate: [0; DEFLATE_WINDOW_BYTES],
            scanline: [0; MAX_SCANLINE_BYTES],
        }
    }
}

impl Default for CoverDecodeWorkspace {
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
}

impl Default for JpegDecodeWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoverDecodeError {
    InvalidPng,
    DimensionsOutOfRange,
}

pub const fn encoded_cover_fits(compressed: u32, uncompressed: u32) -> bool {
    compressed <= MAX_ENCODED_COVER_BYTES && uncompressed <= MAX_ENCODED_COVER_BYTES
}

pub fn decode_png_cover(
    encoded: &[u8],
    output: &mut [u8; COVER_BYTES],
    workspace: &mut CoverDecodeWorkspace,
) -> Result<(), CoverDecodeError> {
    let png =
        ParsedPng::from_bytes(encoded, true, DontDraw).map_err(|_| CoverDecodeError::InvalidPng)?;
    let source_width = png.header.width;
    let source_height = png.header.height;
    if source_width == 0
        || source_height == 0
        || source_width > MAX_COVER_DIMENSION
        || source_height > MAX_COVER_DIMENSION
        || source_width
            .checked_mul(source_height)
            .is_none_or(|pixels| pixels > MAX_DECODED_COVER_PIXELS)
    {
        return Err(CoverDecodeError::DimensionsOutOfRange);
    }
    let scanline_bytes = png_scanline_bytes(encoded, source_width)?;
    if scanline_bytes > workspace.scanline.len() {
        return Err(CoverDecodeError::DimensionsOutOfRange);
    }
    let crop = cover_crop(source_width, source_height);
    output.fill(0xFF);
    png.draw_to_fn::<_, Rgb888>(
        &mut workspace.deflate,
        &mut workspace.scanline[..scanline_bytes],
        |x, y, (alpha, color)| {
            if x < crop.x || y < crop.y || x >= crop.x + crop.width || y >= crop.y + crop.height {
                return Ok(());
            }
            let destination_x = (x - crop.x) * COVER_WIDTH / crop.width;
            let destination_y = (y - crop.y) * COVER_HEIGHT / crop.height;
            if destination_x >= COVER_WIDTH || destination_y >= COVER_HEIGHT {
                return Ok(());
            }
            let foreground = (u32::from(color.r()) * 54
                + u32::from(color.g()) * 183
                + u32::from(color.b()) * 19
                + 128)
                >> 8;
            let alpha = u32::from(alpha);
            let luma = ((foreground * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
            set_pixel(output, destination_x, destination_y, luma);
            Ok(())
        },
    )
    .map_err(|_| CoverDecodeError::InvalidPng)
}

pub fn decode_jpeg_cover(
    encoded: &[u8],
    output: &mut [u8; COVER_BYTES],
    workspace: &mut JpegDecodeWorkspace,
) -> Result<(), CoverDecodeError> {
    let reader = SliceReader { remaining: encoded };
    let mut decoder = JpegDecoder::new(&mut workspace.bytes[..], reader)
        .map_err(|_| CoverDecodeError::InvalidPng)?;
    let scale = jpeg_scale(usize::from(decoder.width()), usize::from(decoder.height()));
    let source_width = usize::from(decoder.width()) >> scale.shift();
    let source_height = usize::from(decoder.height()) >> scale.shift();
    if source_width == 0
        || source_height == 0
        || source_width
            .checked_mul(source_height)
            .is_none_or(|pixels| pixels > MAX_DECODED_COVER_PIXELS)
    {
        return Err(CoverDecodeError::DimensionsOutOfRange);
    }
    let crop = cover_crop(source_width, source_height);
    output.fill(0xFF);
    decoder
        .decode(
            scale,
            PixelFormat::Grayscale,
            &mut |pixels: &[u8], rect: &Rect| {
                let mut source = 0usize;
                for y in usize::from(rect.top)..=usize::from(rect.bottom) {
                    for x in usize::from(rect.left)..=usize::from(rect.right) {
                        let luma = pixels.get(source).copied().unwrap_or(0xFF);
                        source += 1;
                        if x < crop.x
                            || y < crop.y
                            || x >= crop.x + crop.width
                            || y >= crop.y + crop.height
                        {
                            continue;
                        }
                        let destination_x = (x - crop.x) * COVER_WIDTH / crop.width;
                        let destination_y = (y - crop.y) * COVER_HEIGHT / crop.height;
                        if destination_x < COVER_WIDTH && destination_y < COVER_HEIGHT {
                            set_pixel(output, destination_x, destination_y, luma);
                        }
                    }
                }
                true
            },
        )
        .map_err(|_| CoverDecodeError::InvalidPng)
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

fn jpeg_scale(width: usize, height: usize) -> Scale {
    for scale in [Scale::Eighth, Scale::Quarter, Scale::Half] {
        if width >> scale.shift() >= COVER_WIDTH && height >> scale.shift() >= COVER_HEIGHT {
            return scale;
        }
    }
    Scale::None
}

fn set_pixel(output: &mut [u8; COVER_BYTES], x: usize, y: usize, luma: u8) {
    let threshold = BAYER_4X4[y % 4][x % 4] as u32 * 16 + 8;
    let offset = y * (COVER_WIDTH / 8) + x / 8;
    let mask = 0x80 >> (x % 8);
    if u32::from(luma) < threshold {
        output[offset] &= !mask;
    } else {
        output[offset] |= mask;
    }
}

fn png_scanline_bytes(encoded: &[u8], width: usize) -> Result<usize, CoverDecodeError> {
    if encoded.len() < 29 || encoded[28] != 0 {
        return Err(CoverDecodeError::InvalidPng);
    }
    let channels = match encoded[25] {
        0 | 3 => 1usize,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err(CoverDecodeError::InvalidPng),
    };
    let bytes_per_pixel = (usize::from(encoded[24]) * channels).div_ceil(8);
    width
        .checked_mul(bytes_per_pixel)
        .ok_or(CoverDecodeError::DimensionsOutOfRange)
}

pub fn bitmap(bytes: &[u8; COVER_BYTES]) -> MonochromeBitmap<'_> {
    MonochromeBitmap::new(
        Size::new(COVER_WIDTH, COVER_HEIGHT).expect("cover dimensions are non-zero"),
        bytes,
    )
    .expect("the packed cover buffer has the exact required length")
}

#[derive(Clone, Copy)]
struct Crop {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

fn cover_crop(source_width: usize, source_height: usize) -> Crop {
    if source_width * COVER_HEIGHT > source_height * COVER_WIDTH {
        let width = source_height * COVER_WIDTH / COVER_HEIGHT;
        Crop {
            x: (source_width - width) / 2,
            y: 0,
            width,
            height: source_height,
        }
    } else {
        let height = source_width * COVER_HEIGHT / COVER_WIDTH;
        Crop {
            x: 0,
            y: (source_height - height) / 2,
            width: source_width,
            height,
        }
    }
}

const BAYER_4X4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

#[cfg(test)]
mod tests {
    extern crate std;

    use std::boxed::Box;

    use super::{
        COVER_BYTES, CoverDecodeWorkspace, JpegDecodeWorkspace, decode_jpeg_cover,
        decode_png_cover, encoded_cover_fits,
    };
    use crate::{
        device_epub::{DeviceEpub, DevicePackageScratch, MAX_DEVICE_RESOURCE_BYTES},
        zip_stream::{InflateWorkspace, ReadAt, ZipValidationScratch},
    };

    struct SliceFile<'a>(&'a [u8]);

    impl ReadAt for SliceFile<'_> {
        type Error = core::convert::Infallible;

        fn len(&self) -> u32 {
            self.0.len() as u32
        }

        fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
            let start = offset as usize;
            let count = self.0.len().saturating_sub(start).min(output.len());
            output[..count].copy_from_slice(&self.0[start..start + count]);
            Ok(count)
        }
    }

    #[test]
    fn bounds_encoded_cover_work() {
        assert!(encoded_cover_fits(128 * 1024, 128 * 1024));
        assert!(!encoded_cover_fits(128 * 1024 + 1, 1));
        assert!(!encoded_cover_fits(1, 128 * 1024 + 1));
    }

    #[test]
    fn composites_transparent_png_pixels_onto_white() {
        let encoded = include_bytes!("../web/tests/fixtures/transparent.png");
        let mut output = Box::new([0; COVER_BYTES]);
        let mut workspace = Box::new(CoverDecodeWorkspace::new());

        decode_png_cover(encoded, &mut output, &mut workspace).unwrap();

        assert!(output.iter().all(|byte| *byte == 0xFF));
    }

    #[test]
    fn decodes_a_jpeg_directly_into_a_packed_cover() {
        let encoded = include_bytes!("../web/tests/fixtures/cover.jpg");
        let mut output = Box::new([0; COVER_BYTES]);
        let mut workspace = Box::new(JpegDecodeWorkspace::new());

        decode_jpeg_cover(encoded, &mut output, &mut workspace).unwrap();

        assert!(output.iter().any(|byte| *byte != 0xFF));
        assert!(output.iter().any(|byte| *byte != 0x00));
    }

    #[test]
    fn rejects_jpeg_work_above_the_decoded_pixel_budget() {
        let mut encoded = include_bytes!("../web/tests/fixtures/cover.jpg").to_vec();
        let start = encoded
            .windows(2)
            .position(|marker| marker == [0xFF, 0xC0])
            .unwrap();
        encoded[start + 5..start + 7].copy_from_slice(&u16::MAX.to_be_bytes());
        encoded[start + 7..start + 9].copy_from_slice(&u16::MAX.to_be_bytes());
        let mut output = Box::new([0; COVER_BYTES]);
        let mut workspace = Box::new(JpegDecodeWorkspace::new());

        assert_eq!(
            decode_jpeg_cover(&encoded, &mut output, &mut workspace),
            Err(super::CoverDecodeError::DimensionsOutOfRange)
        );
    }

    #[test]
    fn decodes_an_epub_png_directly_into_a_packed_cover() {
        let encoded = include_bytes!("../web/tests/fixtures/minimal.epub");
        let mut zip_scratch = Box::new(ZipValidationScratch::new());
        let mut package_scratch = Box::new(DevicePackageScratch::new());
        let mut inflater = Box::new(InflateWorkspace::new());
        let mut resource = Box::new([0; MAX_DEVICE_RESOURCE_BYTES]);
        let book = DeviceEpub::open(
            SliceFile(encoded),
            &mut zip_scratch,
            &mut package_scratch,
            &mut inflater,
            &mut resource,
        )
        .unwrap();
        let length = book
            .read_cover(&mut resource[..], &mut inflater)
            .unwrap()
            .unwrap();
        let mut output = Box::new([0; COVER_BYTES]);
        let mut workspace = Box::new(CoverDecodeWorkspace::new());

        decode_png_cover(&resource[..length], &mut output, &mut workspace).unwrap();

        assert!(output.iter().any(|byte| *byte != 0xFF));
        assert!(output.iter().any(|byte| *byte != 0x00));
    }
}
