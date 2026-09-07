mod packed;
pub use packed::{PackedBitmap, PackedImage, PixelDepth, READER_DEPTH};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Size {
    width: usize,
    height: usize,
}

impl Size {
    pub fn new(width: usize, height: usize) -> Result<Self, Error> {
        if width == 0 || height == 0 {
            return Err(Error::ZeroDimension);
        }
        width.checked_mul(height).ok_or(Error::DimensionOverflow)?;
        Ok(Self { width, height })
    }

    pub const fn width(self) -> usize {
        self.width
    }

    pub const fn height(self) -> usize {
        self.height
    }

    fn pixels(self) -> usize {
        self.width * self.height
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Region {
    x: usize,
    y: usize,
    size: Size,
}

impl Region {
    pub const fn new(x: usize, y: usize, size: Size) -> Self {
        Self { x, y, size }
    }

    pub const fn x(self) -> usize {
        self.x
    }

    pub const fn y(self) -> usize {
        self.y
    }

    pub const fn size(self) -> Size {
        self.size
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RgbImage<'a> {
    size: Size,
    pixels: &'a [u8],
}

impl<'a> RgbImage<'a> {
    pub fn new(size: Size, pixels: &'a [u8]) -> Result<Self, Error> {
        let expected = size
            .pixels()
            .checked_mul(3)
            .ok_or(Error::DimensionOverflow)?;
        if pixels.len() != expected {
            return Err(Error::InvalidRgbLength {
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Self { size, pixels })
    }

    pub const fn size(&self) -> Size {
        self.size
    }

    fn luma(&self, x: usize, y: usize) -> u8 {
        let offset = (y * self.size.width + x) * 3;
        let red = u32::from(self.pixels[offset]);
        let green = u32::from(self.pixels[offset + 1]);
        let blue = u32::from(self.pixels[offset + 2]);
        ((54 * red + 183 * green + 19 * blue + 128) >> 8) as u8
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScaleMode {
    Contain,
    Cover,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dither {
    None,
    Threshold(u8),
    Ordered4x4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderOptions {
    pub scale: ScaleMode,
    pub dither: Dither,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            scale: ScaleMode::Contain,
            dither: Dither::Ordered4x4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderReport {
    pub source: Size,
    pub target: Size,
    pub scaled: Size,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    ZeroDimension,
    DimensionOverflow,
    InvalidRgbLength { expected: usize, actual: usize },
    WidthNotByteAligned { width: usize },
    InvalidPackedLength { expected: usize, actual: usize },
    RegionOutOfBounds,
}

pub fn render(
    source: &RgbImage<'_>,
    target: &mut PackedImage<'_>,
    options: RenderOptions,
) -> RenderReport {
    target.clear_white();
    render_region(source, target, Region::new(0, 0, target.size()), options)
        .expect("the full target is always a valid render region")
}

pub fn render_region(
    source: &RgbImage<'_>,
    target: &mut PackedImage<'_>,
    region: Region,
    options: RenderOptions,
) -> Result<RenderReport, Error> {
    let right = region
        .x
        .checked_add(region.size.width)
        .ok_or(Error::RegionOutOfBounds)?;
    let bottom = region
        .y
        .checked_add(region.size.height)
        .ok_or(Error::RegionOutOfBounds)?;
    if right > target.size().width || bottom > target.size().height {
        return Err(Error::RegionOutOfBounds);
    }

    let scaled = scaled_size(source.size, region.size, options.scale);
    let left = (region.size.width as i128 - scaled.width as i128) / 2;
    let top = (region.size.height as i128 - scaled.height as i128) / 2;

    for region_y in 0..region.size.height {
        let scaled_y = region_y as i128 - top;
        for region_x in 0..region.size.width {
            let scaled_x = region_x as i128 - left;
            let luma = if (0..scaled.width as i128).contains(&scaled_x)
                && (0..scaled.height as i128).contains(&scaled_y)
            {
                sample_bilinear(source, scaled_x as usize, scaled_y as usize, scaled)
            } else {
                255
            };
            target.set_luma_dithered(
                region.x + region_x,
                region.y + region_y,
                luma,
                options.dither,
            );
        }
    }

    Ok(RenderReport {
        source: source.size,
        target: region.size,
        scaled,
    })
}

fn scaled_size(source: Size, target: Size, mode: ScaleMode) -> Size {
    let width_limited = (target.width as u128) * (source.height as u128)
        <= (target.height as u128) * (source.width as u128);
    let scale_to_width = match mode {
        ScaleMode::Contain => width_limited,
        ScaleMode::Cover => !width_limited,
    };

    if scale_to_width {
        Size {
            width: target.width,
            height: rounded_ratio(source.height, target.width, source.width),
        }
    } else {
        Size {
            width: rounded_ratio(source.width, target.height, source.height),
            height: target.height,
        }
    }
}

fn rounded_ratio(value: usize, numerator: usize, denominator: usize) -> usize {
    let result =
        ((value as u128) * (numerator as u128) + (denominator as u128 / 2)) / denominator as u128;
    usize::try_from(result).unwrap_or(usize::MAX).max(1)
}

fn sample_bilinear(source: &RgbImage<'_>, x: usize, y: usize, scaled: Size) -> u8 {
    let (x0, x1, x_weight) = sample_axis(x, scaled.width, source.size.width);
    let (y0, y1, y_weight) = sample_axis(y, scaled.height, source.size.height);

    let top = interpolate(source.luma(x0, y0), source.luma(x1, y0), x_weight);
    let bottom = interpolate(source.luma(x0, y1), source.luma(x1, y1), x_weight);
    interpolate(top, bottom, y_weight)
}

fn sample_axis(position: usize, scaled: usize, source: usize) -> (usize, usize, u16) {
    let center = ((2 * position as u128 + 1) * source as u128 * 256) / (2 * scaled as u128);
    let coordinate = center as i128 - 128;
    let maximum = ((source - 1) * 256) as i128;
    let clamped = coordinate.clamp(0, maximum) as usize;
    let first = clamped / 256;
    let second = (first + 1).min(source - 1);
    (first, second, (clamped % 256) as u16)
}

fn interpolate(first: u8, second: u8, weight: u16) -> u8 {
    let inverse = 256 - u32::from(weight);
    let value = u32::from(first) * inverse + u32::from(second) * u32::from(weight);
    ((value + 128) / 256) as u8
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec;

    use super::{
        Dither, Error, PackedBitmap, PackedImage, Region, RenderOptions, RgbImage, ScaleMode, Size,
        render, render_region,
    };

    #[test]
    fn constructors_reject_invalid_buffer_shapes() {
        assert_eq!(Size::new(0, 1), Err(Error::ZeroDimension));
        let size = Size::new(8, 2).unwrap();
        assert!(matches!(
            RgbImage::new(size, &[0; 2]),
            Err(Error::InvalidRgbLength {
                expected: 48,
                actual: 2,
            })
        ));
        assert!(matches!(
            PackedBitmap::monochrome(size, &[0; 1]),
            Err(Error::InvalidPackedLength {
                expected: 2,
                actual: 1,
            })
        ));
        let mut bytes = [0; 1];
        assert!(matches!(
            PackedImage::monochrome(size, &mut bytes),
            Err(Error::InvalidPackedLength {
                expected: 2,
                actual: 1,
            })
        ));
    }

    #[test]
    fn threshold_converts_rgb_to_luma() {
        let size = Size::new(8, 1).unwrap();
        let mut rgb = vec![255; 8 * 3];
        rgb[..3].copy_from_slice(&[255, 0, 0]);
        rgb[3..6].copy_from_slice(&[0, 255, 0]);
        let source = RgbImage::new(size, &rgb).unwrap();
        let mut bytes = [0; 1];
        let mut target = PackedImage::monochrome(size, &mut bytes).unwrap();

        render(
            &source,
            &mut target,
            RenderOptions {
                scale: ScaleMode::Contain,
                dither: Dither::Threshold(128),
            },
        );

        assert!(target.pixel_is_black(0, 0));
        assert!(!target.pixel_is_black(1, 0));
        assert!(!target.pixel_is_black(7, 0));
    }

    #[test]
    fn contain_centers_the_whole_source_with_white_bars() {
        let source_size = Size::new(2, 2).unwrap();
        let source_bytes = [0; 12];
        let source = RgbImage::new(source_size, &source_bytes).unwrap();
        let target_size = Size::new(8, 16).unwrap();
        let mut bytes = [0; 16];
        let mut target = PackedImage::monochrome(target_size, &mut bytes).unwrap();

        let report = render(
            &source,
            &mut target,
            RenderOptions {
                scale: ScaleMode::Contain,
                dither: Dither::Threshold(128),
            },
        );

        assert_eq!(report.scaled, Size::new(8, 8).unwrap());
        assert!(!target.pixel_is_black(4, 3));
        assert!(target.pixel_is_black(4, 4));
        assert!(target.pixel_is_black(4, 11));
        assert!(!target.pixel_is_black(4, 12));
    }

    #[test]
    fn cover_fills_the_target_and_crops_the_source() {
        let source_size = Size::new(2, 1).unwrap();
        let mut source_bytes = [255; 6];
        source_bytes[..3].fill(0);
        let source = RgbImage::new(source_size, &source_bytes).unwrap();
        let target_size = Size::new(8, 8).unwrap();
        let mut bytes = [0; 8];
        let mut target = PackedImage::monochrome(target_size, &mut bytes).unwrap();

        let report = render(
            &source,
            &mut target,
            RenderOptions {
                scale: ScaleMode::Cover,
                dither: Dither::Threshold(128),
            },
        );

        assert_eq!(report.scaled, Size::new(16, 8).unwrap());
        assert!(target.pixel_is_black(0, 4));
        assert!(!target.pixel_is_black(7, 4));
    }

    #[test]
    fn region_rendering_changes_only_the_requested_rectangle() {
        let source_size = Size::new(1, 1).unwrap();
        let source = RgbImage::new(source_size, &[0, 0, 0]).unwrap();
        let target_size = Size::new(16, 8).unwrap();
        let mut bytes = [0xFF; 16];
        let mut target = PackedImage::monochrome(target_size, &mut bytes).unwrap();

        render_region(
            &source,
            &mut target,
            Region::new(4, 2, Size::new(8, 4).unwrap()),
            RenderOptions {
                scale: ScaleMode::Cover,
                dither: Dither::Threshold(128),
            },
        )
        .unwrap();

        assert!(!target.pixel_is_black(3, 3));
        assert!(target.pixel_is_black(4, 2));
        assert!(target.pixel_is_black(11, 5));
        assert!(!target.pixel_is_black(12, 5));
        assert!(!target.pixel_is_black(7, 6));
    }

    #[test]
    fn region_rendering_rejects_out_of_bounds_rectangles() {
        let source = RgbImage::new(Size::new(1, 1).unwrap(), &[0, 0, 0]).unwrap();
        let target_size = Size::new(8, 8).unwrap();
        let mut bytes = [0xFF; 8];
        let mut target = PackedImage::monochrome(target_size, &mut bytes).unwrap();

        assert_eq!(
            render_region(
                &source,
                &mut target,
                Region::new(1, 1, Size::new(8, 8).unwrap()),
                RenderOptions::default(),
            ),
            Err(Error::RegionOutOfBounds)
        );
    }

    #[test]
    fn ordered_dither_is_balanced_for_middle_gray() {
        let size = Size::new(8, 4).unwrap();
        let rgb = [128; 8 * 4 * 3];
        let source = RgbImage::new(size, &rgb).unwrap();
        let mut bytes = [0; 4];
        let mut target = PackedImage::monochrome(size, &mut bytes).unwrap();

        render(&source, &mut target, RenderOptions::default());

        let black = (0..4)
            .flat_map(|y| (0..8).map(move |x| (x, y)))
            .filter(|&(x, y)| target.pixel_is_black(x, y))
            .count();
        assert_eq!(black, 16);
    }
}
