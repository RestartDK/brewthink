extern crate std;
use super::{PngDecodeWorkspace, decode_png};
use crate::image::{Dither, PackedImage, READER_DEPTH, RenderOptions, ScaleMode, Size};
use std::boxed::Box;

#[test]
fn native_cover_decode_preserves_one_pixel_detail_across_the_full_frame() {
    let encoded = include_bytes!("../../web/tests/fixtures/navigation-cover.png");
    let size = Size::new(480, 800).unwrap();
    let mut pixels = std::vec![0xff; READER_DEPTH.byte_len(size).unwrap()];
    let mut frame = PackedImage::new(size, READER_DEPTH, &mut pixels).unwrap();
    let mut workspace = Box::new(PngDecodeWorkspace::new());
    decode_png(
        encoded,
        &mut frame,
        RenderOptions {
            scale: ScaleMode::Contain,
            dither: Dither::None,
        },
        &mut workspace,
    )
    .unwrap();
    for y in 0..800 {
        for x in 0..480 {
            let expected = if x % 8 == 0 || (x < 240 && y % 8 == 0) {
                0
            } else {
                255
            };
            assert_eq!(frame.luma(x, y), expected, "pixel {x},{y}");
        }
    }
}
