use super::{Dither, Error, Size};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PixelDepth {
    Monochrome,
    #[default]
    Four,
}

impl PixelDepth {
    pub const fn bits(self) -> usize {
        match self {
            Self::Monochrome => 1,
            Self::Four => 2,
        }
    }

    pub const fn levels(self) -> u8 {
        1 << self.bits()
    }

    pub const fn from_bits(bits: usize) -> Option<Self> {
        match bits {
            1 => Some(Self::Monochrome),
            2 => Some(Self::Four),
            _ => None,
        }
    }

    pub fn byte_len(self, size: Size) -> Result<usize, Error> {
        if !size.width().is_multiple_of(8) {
            return Err(Error::WidthNotByteAligned {
                width: size.width(),
            });
        }
        (size.pixels() / 8)
            .checked_mul(self.bits())
            .ok_or(Error::DimensionOverflow)
    }
}

pub const READER_DEPTH: PixelDepth = PixelDepth::Four;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackedBitmap<'a> {
    size: Size,
    depth: PixelDepth,
    bytes: &'a [u8],
}

impl<'a> PackedBitmap<'a> {
    pub fn new(size: Size, depth: PixelDepth, bytes: &'a [u8]) -> Result<Self, Error> {
        validate_shape(size, depth, bytes.len())?;
        Ok(Self { size, depth, bytes })
    }

    pub fn monochrome(size: Size, bytes: &'a [u8]) -> Result<Self, Error> {
        Self::new(size, PixelDepth::Monochrome, bytes)
    }

    pub const fn size(self) -> Size {
        self.size
    }
    pub const fn depth(self) -> PixelDepth {
        self.depth
    }
    pub const fn as_bytes(self) -> &'a [u8] {
        self.bytes
    }

    pub fn level(self, x: usize, y: usize) -> u8 {
        assert!(x < self.size.width() && y < self.size.height());
        let offset = y * (self.size.width() / 8) + x / 8;
        let mask = 0x80 >> (x % 8);
        let plane_bytes = self.size.pixels() / 8;
        (0..self.depth.bits()).fold(0, |level, bit| {
            level | (u8::from(self.bytes[bit * plane_bytes + offset] & mask != 0) << bit)
        })
    }

    pub fn luma(self, x: usize, y: usize) -> u8 {
        (u16::from(self.level(x, y)) * 255 / u16::from(self.depth.levels() - 1)) as u8
    }

    pub fn pixel_is_black(self, x: usize, y: usize) -> bool {
        self.level(x, y) == 0
    }

    pub fn is_monochrome(self) -> bool {
        let plane_bytes = self.size.pixels() / 8;
        self.bytes
            .chunks_exact(plane_bytes)
            .all(|plane| plane == &self.bytes[..plane_bytes])
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct PackedImage<'a> {
    size: Size,
    depth: PixelDepth,
    bytes: &'a mut [u8],
}

impl<'a> PackedImage<'a> {
    pub fn new(size: Size, depth: PixelDepth, bytes: &'a mut [u8]) -> Result<Self, Error> {
        validate_shape(size, depth, bytes.len())?;
        Ok(Self { size, depth, bytes })
    }

    pub fn monochrome(size: Size, bytes: &'a mut [u8]) -> Result<Self, Error> {
        Self::new(size, PixelDepth::Monochrome, bytes)
    }

    pub const fn size(&self) -> Size {
        self.size
    }
    pub const fn depth(&self) -> PixelDepth {
        self.depth
    }
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes
    }
    pub fn bitmap(&self) -> PackedBitmap<'_> {
        PackedBitmap {
            size: self.size,
            depth: self.depth,
            bytes: self.bytes,
        }
    }
    pub fn pixel_is_black(&self, x: usize, y: usize) -> bool {
        self.bitmap().pixel_is_black(x, y)
    }
    pub fn luma(&self, x: usize, y: usize) -> u8 {
        self.bitmap().luma(x, y)
    }

    pub fn clear_white(&mut self) {
        self.bytes.fill(0xFF);
    }

    pub fn set_luma(&mut self, x: usize, y: usize, luma: u8) {
        self.set_luma_dithered(x, y, luma, Dither::None);
    }

    pub(crate) fn set_luma_dithered(&mut self, x: usize, y: usize, luma: u8, dither: Dither) {
        let maximum = u16::from(self.depth.levels() - 1);
        let scaled = u16::from(luma) * maximum;
        let level = match dither {
            Dither::None => (scaled + 127) / 255,
            Dither::Threshold(threshold) if self.depth == PixelDepth::Monochrome => {
                u16::from(luma >= threshold)
            }
            Dither::Threshold(_) => (scaled + 127) / 255,
            Dither::Ordered4x4 => {
                const BAYER: [[u8; 4]; 4] =
                    [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
                if self.depth == PixelDepth::Monochrome {
                    u16::from(luma >= BAYER[y % 4][x % 4] * 16 + 8)
                } else {
                    (scaled / 255
                        + u16::from(scaled % 255 > u16::from(BAYER[y % 4][x % 4]) * 16 + 8))
                    .min(maximum)
                }
            }
        } as u8;
        assert!(x < self.size.width() && y < self.size.height());
        let offset = y * (self.size.width() / 8) + x / 8;
        let mask = 0x80 >> (x % 8);
        let plane_bytes = self.size.pixels() / 8;
        for bit in 0..self.depth.bits() {
            let byte = &mut self.bytes[bit * plane_bytes + offset];
            *byte = (*byte & !mask) | if level & (1 << bit) != 0 { mask } else { 0 };
        }
    }
}

fn validate_shape(size: Size, depth: PixelDepth, actual: usize) -> Result<(), Error> {
    let expected = depth.byte_len(size)?;
    if actual != expected {
        return Err(Error::InvalidPackedLength { expected, actual });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec;

    #[test]
    fn every_tone_round_trips_in_lsb_first_bitplanes() {
        let size = Size::new(8, 2).unwrap();
        for depth in [PixelDepth::Monochrome, PixelDepth::Four] {
            let mut bytes = vec![0; depth.byte_len(size).unwrap()];
            let mut image = PackedImage::new(size, depth, &mut bytes).unwrap();
            for x in 0..8 {
                let level = x as u8 % depth.levels();
                image.set_luma(
                    x,
                    0,
                    (u16::from(level) * 255 / u16::from(depth.levels() - 1)) as u8,
                );
                assert_eq!(image.bitmap().level(x, 0), level);
            }
            image.clear_white();
            assert!(image.as_bytes().iter().all(|&byte| byte == 255));
            assert!(image.bitmap().is_monochrome());
            image.set_luma(7, 1, 0);
            assert!(image.bitmap().is_monochrome());
            if depth != PixelDepth::Monochrome {
                image.set_luma(0, 1, 128);
                assert!(!image.bitmap().is_monochrome());
            }
        }
    }

    #[test]
    fn near_white_has_no_bayer_speckles_without_dither() {
        let mut bytes = [0; 16];
        let mut image =
            PackedImage::new(Size::new(8, 8).unwrap(), PixelDepth::Four, &mut bytes).unwrap();
        for y in 0..8 {
            for x in 0..8 {
                image.set_luma(x, y, 240);
            }
        }
        assert!(image.as_bytes().iter().all(|&byte| byte == 255));
    }

    #[test]
    fn rejects_invalid_depth_and_buffer_shapes() {
        assert_eq!(PixelDepth::from_bits(3), None);
        assert_eq!(PixelDepth::from_bits(4), None);
        assert!(PackedBitmap::new(Size::new(8, 1).unwrap(), PixelDepth::Four, &[0]).is_err());
        assert!(PackedBitmap::new(Size::new(7, 1).unwrap(), PixelDepth::Four, &[0; 2]).is_err());
        assert_eq!(
            PixelDepth::Four.byte_len(Size::new(480, 800).unwrap()),
            Ok(96_000)
        );
    }
}
