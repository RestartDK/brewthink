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
                self.image.set_luma(x, y, color.luma());
            }
        }
        Ok(())
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
