extern crate std;
use super::{PngDecodeWorkspace, decode_png};
use crate::image::{MonochromeImage, RenderOptions, ScaleMode, Size};
use std::boxed::Box;

#[test]
fn native_cover_decode_preserves_one_pixel_detail_across_the_full_frame() {
    let encoded = include_bytes!("../../web/tests/fixtures/navigation-cover.png");
    let mut pixels = std::vec![0xff; 48_000];
    let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut pixels).unwrap();
    let mut workspace = Box::new(PngDecodeWorkspace::new());
    decode_png(
        encoded,
        &mut frame,
        RenderOptions {
            scale: ScaleMode::Contain,
            ..RenderOptions::default()
        },
        &mut workspace,
    )
    .unwrap();
    for y in 0..800 {
        for x in 0..480 {
            assert_eq!(
                frame.pixel_is_black(x, y),
                x % 8 == 0 || (x < 240 && y % 8 == 0),
                "pixel {x},{y}"
            );
        }
    }
}
