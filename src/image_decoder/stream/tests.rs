extern crate std;

use core::{cell::Cell, convert::Infallible};
use image::ImageEncoder;
use std::{boxed::Box, vec, vec::Vec};

use super::*;
use crate::image::{Dither, PixelDepth, ScaleMode};

struct Source<'a> {
    bytes: &'a [u8],
    largest: Cell<usize>,
}

impl ReadAt for Source<'_> {
    type Error = Infallible;

    fn len(&self) -> u32 {
        self.bytes.len() as u32
    }

    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        self.largest.set(self.largest.get().max(output.len()));
        let offset = offset as usize;
        let count = output
            .len()
            .min(7)
            .min(self.bytes.len().saturating_sub(offset));
        output[..count].copy_from_slice(&self.bytes[offset..offset + count]);
        Ok(count)
    }
}

fn source(bytes: &[u8]) -> Source<'_> {
    Source {
        bytes,
        largest: Cell::new(0),
    }
}

fn options() -> RenderOptions {
    RenderOptions {
        scale: ScaleMode::Contain,
        dither: Dither::None,
    }
}

#[test]
fn large_png_streams_short_reads_into_the_same_pixels_as_the_slice_decoder() {
    let mut state = 73u32;
    let pixels: Vec<u8> = (0..1024 * 256 * 3)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(&pixels, 1024, 256, image::ExtendedColorType::Rgb8)
        .unwrap();
    assert!(encoded.len() > 128 * 1024);
    let source = source(&encoded);
    let size = Size::new(256, 64).unwrap();
    let mut streamed = vec![0; PixelDepth::Four.byte_len(size).unwrap()];
    let mut expected = streamed.clone();
    let mut workspace = Box::new(PngWorkspace::new());
    decode_png(
        &source,
        &mut PackedImage::new(size, PixelDepth::Four, &mut streamed).unwrap(),
        options(),
        &mut workspace,
    )
    .unwrap();
    super::super::decode_png(
        &encoded,
        &mut PackedImage::new(size, PixelDepth::Four, &mut expected).unwrap(),
        options(),
        &mut Box::new(super::super::PngDecodeWorkspace::new()),
    )
    .unwrap();
    assert_eq!(streamed, expected);
    assert!(source.largest.get() <= 1024);
}

#[test]
fn stream_decoder_composites_alpha_and_reuses_workspace_without_old_rows() {
    let mut workspace = Box::new(PngWorkspace::new());
    let size = Size::new(64, 64).unwrap();
    let mut output = vec![0; PixelDepth::Four.byte_len(size).unwrap()];
    for encoded in [
        include_bytes!("../../../web/tests/fixtures/transparent.png").as_slice(),
        include_bytes!("../../../web/tests/fixtures/gray-ramp.png").as_slice(),
    ] {
        let mut expected = output.clone();
        super::super::decode_png(
            encoded,
            &mut PackedImage::new(size, PixelDepth::Four, &mut expected).unwrap(),
            options(),
            &mut Box::new(super::super::PngDecodeWorkspace::new()),
        )
        .unwrap();
        decode_png(
            &source(encoded),
            &mut PackedImage::new(size, PixelDepth::Four, &mut output).unwrap(),
            options(),
            &mut workspace,
        )
        .unwrap();
        assert_eq!(output, expected);
    }
}

#[test]
fn damaged_and_truncated_png_never_succeed() {
    let original = include_bytes!("../../../web/tests/fixtures/gray-ramp.png");
    let size = Size::new(64, 64).unwrap();
    let mut pixels = vec![0; PixelDepth::Four.byte_len(size).unwrap()];
    let mut workspace = Box::new(PngWorkspace::new());
    let mut corrupt = original.to_vec();
    let index = corrupt
        .windows(4)
        .position(|chunk| chunk == b"IDAT")
        .unwrap()
        + 5;
    corrupt[index] ^= 1;
    for encoded in [
        &original[..8],
        &original[..original.len() - 1],
        &corrupt[..],
    ] {
        assert!(
            decode_png(
                &source(encoded),
                &mut PackedImage::new(size, PixelDepth::Four, &mut pixels).unwrap(),
                options(),
                &mut workspace
            )
            .is_err()
        );
    }
}

#[test]
fn jpeg_accepts_short_sd_reads_without_an_encoded_buffer() {
    let encoded = include_bytes!("../../../web/tests/fixtures/cover.jpg");
    let source = source(encoded);
    let size = Size::new(176, 264).unwrap();
    let mut expected = vec![0; PixelDepth::Four.byte_len(size).unwrap()];
    let mut actual = expected.clone();
    let mut work = Box::new(super::super::JpegDecodeWorkspace::new());
    super::super::decode_jpeg(
        encoded,
        &mut PackedImage::new(size, PixelDepth::Four, &mut expected).unwrap(),
        options(),
        &mut work,
    )
    .unwrap();
    super::super::decode_jpeg_reader(
        Input::new(&source),
        &mut PackedImage::new(size, PixelDepth::Four, &mut actual).unwrap(),
        options(),
        &mut work,
    )
    .unwrap();
    assert_eq!(actual, expected);
    assert!(source.largest.get() <= 1024);
    assert!(dimensions(&source).is_ok());
}
