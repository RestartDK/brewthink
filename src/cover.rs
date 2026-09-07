use crate::{
    image::{MonochromeBitmap, MonochromeImage, RenderOptions, ScaleMode, Size},
    image_decoder::{ImageDecodeError, decode_jpeg, decode_png},
};

pub use crate::image_decoder::{JpegDecodeWorkspace, PngDecodeWorkspace};

pub const COVER_WIDTH: usize = 176;
pub const COVER_HEIGHT: usize = 264;
pub const COVER_BYTES: usize = COVER_WIDTH * COVER_HEIGHT / 8;
pub const SHELF_COVER_WIDTH: usize = COVER_WIDTH / 2;
pub const SHELF_COVER_HEIGHT: usize = COVER_HEIGHT / 2;
pub const SHELF_COVER_BYTES: usize = SHELF_COVER_WIDTH * SHELF_COVER_HEIGHT / 8;
pub const MAX_ENCODED_COVER_BYTES: u32 = 128 * 1024;

pub type CoverDecodeWorkspace = PngDecodeWorkspace;
pub type CoverDecodeError = ImageDecodeError;

pub const fn encoded_cover_fits(compressed: u32, uncompressed: u32) -> bool {
    compressed <= MAX_ENCODED_COVER_BYTES && uncompressed <= MAX_ENCODED_COVER_BYTES
}

pub fn decode_png_cover(
    encoded: &[u8],
    output: &mut [u8; COVER_BYTES],
    workspace: &mut CoverDecodeWorkspace,
) -> Result<(), CoverDecodeError> {
    let mut target = MonochromeImage::new(cover_size(), output)
        .expect("the packed cover buffer has the exact required length");
    decode_png(
        encoded,
        &mut target,
        RenderOptions {
            scale: ScaleMode::Cover,
            ..RenderOptions::default()
        },
        workspace,
    )?;
    Ok(())
}

pub fn decode_jpeg_cover(
    encoded: &[u8],
    output: &mut [u8; COVER_BYTES],
    workspace: &mut JpegDecodeWorkspace,
) -> Result<(), CoverDecodeError> {
    let mut target = MonochromeImage::new(cover_size(), output)
        .expect("the packed cover buffer has the exact required length");
    decode_jpeg(
        encoded,
        &mut target,
        RenderOptions {
            scale: ScaleMode::Cover,
            ..RenderOptions::default()
        },
        workspace,
    )?;
    Ok(())
}

pub fn bitmap(bytes: &[u8; COVER_BYTES]) -> MonochromeBitmap<'_> {
    MonochromeBitmap::new(cover_size(), bytes)
        .expect("the packed cover buffer has the exact required length")
}

pub fn downsample_cover(source: &[u8; COVER_BYTES], output: &mut [u8; SHELF_COVER_BYTES]) {
    let source = bitmap(source);
    output.fill(0xFF);
    for y in 0..SHELF_COVER_HEIGHT {
        for x in 0..SHELF_COVER_WIDTH {
            let source_x = x * 2;
            let source_y = y * 2;
            let black = usize::from(source.pixel_is_black(source_x, source_y))
                + usize::from(source.pixel_is_black(source_x + 1, source_y))
                + usize::from(source.pixel_is_black(source_x, source_y + 1))
                + usize::from(source.pixel_is_black(source_x + 1, source_y + 1));
            if black >= 2 {
                let pixel = y * SHELF_COVER_WIDTH + x;
                output[pixel / 8] &= !(0x80 >> (pixel % 8));
            }
        }
    }
}

pub fn shelf_bitmap(bytes: &[u8; SHELF_COVER_BYTES]) -> MonochromeBitmap<'_> {
    MonochromeBitmap::new(
        Size::new(SHELF_COVER_WIDTH, SHELF_COVER_HEIGHT)
            .expect("the shelf cover dimensions are non-zero"),
        bytes,
    )
    .expect("the shelf cover buffer matches its dimensions")
}

fn cover_size() -> Size {
    Size::new(COVER_WIDTH, COVER_HEIGHT).expect("cover dimensions are non-zero")
}

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
    fn shelf_downsampling_counts_all_four_pixels_including_ties() {
        for mask in 0u8..16 {
            let mut source = [0xff; COVER_BYTES];
            for (bit, (byte, flag)) in [(0, 0x80), (0, 0x40), (22, 0x80), (22, 0x40)]
                .into_iter()
                .enumerate()
            {
                if mask & (1 << bit) != 0 {
                    source[byte] &= !flag;
                }
            }
            let mut output = [0; super::SHELF_COVER_BYTES];
            super::downsample_cover(&source, &mut output);
            assert_eq!(
                output[0],
                if mask.count_ones() >= 2 { 0x7f } else { 0xff },
                "mask {mask:04b}"
            );
            assert!(output[1..].iter().all(|byte| *byte == 0xff));
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
