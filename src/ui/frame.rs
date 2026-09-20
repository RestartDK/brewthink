use core::{convert::Infallible, fmt::Write};

use embedded_graphics::{
    Pixel,
    geometry::{OriginDimensions, Size as GraphicsSize},
    pixelcolor::{Gray8, GrayColor},
    prelude::DrawTarget,
};

use crate::image::PackedImage;

pub struct FrameTarget<'target, 'bytes> {
    image: &'target mut PackedImage<'bytes>,
}

impl<'target, 'bytes> FrameTarget<'target, 'bytes> {
    pub fn new(image: &'target mut PackedImage<'bytes>) -> Self {
        Self { image }
    }
}

impl OriginDimensions for FrameTarget<'_, '_> {
    fn size(&self) -> GraphicsSize {
        GraphicsSize::new(
            self.image.size().width() as u32,
            self.image.size().height() as u32,
        )
    }
}

impl DrawTarget for FrameTarget<'_, '_> {
    type Color = Gray8;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let width = self.image.size().width();
        let height = self.image.size().height();
        for Pixel(point, color) in pixels {
            if let (Ok(x), Ok(y)) = (usize::try_from(point.x), usize::try_from(point.y))
                && x < width
                && y < height
            {
                const THRESHOLDS: [[u8; 2]; 2] = [[224, 96], [32, 160]];
                let luma = if color.luma() >= THRESHOLDS[y % 2][x % 2] {
                    255
                } else {
                    0
                };
                self.image.set_luma(x, y, luma);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{PixelDepth, Size};
    use embedded_graphics::geometry::Point;

    #[test]
    fn ui_tones_are_binary_with_ordered_coverage() {
        let size = Size::new(8, 8).unwrap();
        for depth in [PixelDepth::Monochrome, PixelDepth::Four] {
            for (tone, expected_black) in [(0, 64), (85, 48), (170, 16), (255, 0)] {
                let mut bytes = [0xff; 16];
                let len = depth.byte_len(size).unwrap();
                let mut image = PackedImage::new(size, depth, &mut bytes[..len]).unwrap();
                FrameTarget::new(&mut image)
                    .clear(Gray8::new(tone))
                    .unwrap();
                assert!(image.bitmap().is_monochrome());
                let black = (0..8)
                    .flat_map(|y| (0..8).map(move |x| (x, y)))
                    .filter(|&(x, y)| image.pixel_is_black(x, y))
                    .count();
                assert_eq!(black, expected_black);
            }
        }
    }

    #[test]
    fn clipped_and_fragmented_ui_drawing_keeps_the_screen_anchored_pattern() {
        let size = Size::new(8, 8).unwrap();
        let mut bytes = [0xff; 16];
        let mut image = PackedImage::new(size, PixelDepth::Four, &mut bytes).unwrap();
        let mut target = FrameTarget::new(&mut image);
        for column in [-1..3, 3..9] {
            target
                .draw_iter((-1..9).flat_map(|y| {
                    column
                        .clone()
                        .map(move |x| Pixel(Point::new(x, y), Gray8::new(170)))
                }))
                .unwrap();
        }
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(
                    image.luma(x, y),
                    if x % 2 == 0 && y % 2 == 0 { 0 } else { 255 }
                );
            }
        }
    }

    #[test]
    fn drawing_ui_does_not_convert_untouched_image_pixels() {
        let size = Size::new(8, 8).unwrap();
        let mut bytes = [0xff; 16];
        let mut image = PackedImage::new(size, PixelDepth::Four, &mut bytes).unwrap();
        for x in 0..8 {
            image.set_luma(x, 0, [0, 85, 170, 255][x % 4]);
        }
        FrameTarget::new(&mut image)
            .draw_iter([
                Pixel(Point::new(0, 2), Gray8::new(170)),
                Pixel(Point::new(1, 2), Gray8::new(170)),
            ])
            .unwrap();
        assert_eq!(image.luma(0, 2), 0);
        assert_eq!(image.luma(1, 2), 255);
        for x in 0..8 {
            assert_eq!(image.luma(x, 0), [0, 85, 170, 255][x % 4]);
        }
    }
}

pub struct FixedText<const CAPACITY: usize> {
    bytes: [u8; CAPACITY],
    length: usize,
}

impl<const CAPACITY: usize> FixedText<CAPACITY> {
    pub const fn new() -> Self {
        Self {
            bytes: [0; CAPACITY],
            length: 0,
        }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.length]).expect("fixed text only stores UTF-8")
    }
}

impl<const CAPACITY: usize> Default for FixedText<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAPACITY: usize> Write for FixedText<CAPACITY> {
    fn write_str(&mut self, value: &str) -> core::fmt::Result {
        let end = self
            .length
            .checked_add(value.len())
            .ok_or(core::fmt::Error)?;
        if end > CAPACITY {
            return Err(core::fmt::Error);
        }
        self.bytes[self.length..end].copy_from_slice(value.as_bytes());
        self.length = end;
        Ok(())
    }
}
