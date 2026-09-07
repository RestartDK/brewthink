extern crate std;

use super::{DrawerSurface, FrameTarget};
use crate::image::{MonochromeImage, Size};
use embedded_graphics::Drawable;

#[test]
fn drawer_preserves_rounded_corners_and_fades_the_page_behind_it() {
    let mut bytes = std::vec![0; 48_000];
    let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();
    DrawerSurface::new(304)
        .draw(&mut FrameTarget::new(&mut frame))
        .unwrap();
    let image = &frame;
    assert!(!image.pixel_is_black(60, 100));
    assert!(image.pixel_is_black(61, 100));
    assert!(image.pixel_is_black(13, 306));
    assert!(image.pixel_is_black(240, 304));
    for y in 312..334 {
        for x in 64..416 {
            assert!(
                !image.pixel_is_black(x, y),
                "unexpected drag affordance at {x},{y}"
            );
        }
    }
    assert!(!image.pixel_is_black(240, 340));
    assert!(image.pixel_is_black(240, 791));
}
