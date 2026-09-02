use core::{convert::Infallible, fmt::Write};

use embedded_graphics::{
    Drawable, Pixel,
    draw_target::DrawTargetExt,
    geometry::{OriginDimensions, Point, Size as GraphicsSize},
    mono_font::{MonoTextStyle, ascii::FONT_6X10, ascii::FONT_9X18_BOLD},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Primitive},
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};

use crate::{
    image::MonochromeImage,
    power::{BatteryLevel, BatteryStatus},
};

pub const FRAME_WIDTH: usize = 480;
pub const FRAME_HEIGHT: usize = 800;
pub const CONTENT_LEFT: i32 = 18;
pub const CONTENT_WIDTH: u32 = 444;
pub const APP_BAR_RULE_Y: i32 = 58;
pub const CONTENT_TOP: usize = 72;

pub const fn chrome_style() -> MonoTextStyle<'static, BinaryColor> {
    MonoTextStyle::new(&FONT_6X10, BinaryColor::On)
}

pub const fn brand_style() -> MonoTextStyle<'static, BinaryColor> {
    MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::On)
}

pub fn draw_app_bar(target: &mut MonochromeImage<'_>, section: &str, battery: BatteryStatus) {
    let mut display = FrameTarget::new(target);
    Text::with_baseline(
        "BREWTHINK",
        Point::new(CONTENT_LEFT, 14),
        brand_style(),
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();

    let section_clip = Rectangle::new(Point::new(CONTENT_LEFT, 39), GraphicsSize::new(330, 12));
    Text::with_baseline(
        section,
        Point::new(CONTENT_LEFT, 39),
        chrome_style(),
        Baseline::Top,
    )
    .draw(&mut display.clipped(&section_clip))
    .ok();

    Rectangle::new(
        Point::new(CONTENT_LEFT, APP_BAR_RULE_Y),
        GraphicsSize::new(CONTENT_WIDTH, 2),
    )
    .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
    .draw(&mut display)
    .ok();

    draw_battery(target, battery);
}

pub fn draw_footer_rule(target: &mut MonochromeImage<'_>, y: i32) {
    Rectangle::new(
        Point::new(CONTENT_LEFT, y),
        GraphicsSize::new(CONTENT_WIDTH, 1),
    )
    .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
    .draw(&mut FrameTarget::new(target))
    .ok();
}

fn draw_battery(target: &mut MonochromeImage<'_>, battery: BatteryStatus) {
    let mut display = FrameTarget::new(target);
    let outline = Rectangle::new(Point::new(374, 20), GraphicsSize::new(27, 13));
    outline
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut display)
        .ok();
    Rectangle::new(Point::new(401, 24), GraphicsSize::new(3, 5))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut display)
        .ok();

    let mut label = FixedText::<12>::new();
    match battery.level() {
        BatteryLevel::Unknown => {
            label.write_str("--%").ok();
        }
        BatteryLevel::Percent(percent) => {
            let fill = usize::from(percent.get()) * 23 / 100;
            if fill > 0 {
                Rectangle::new(Point::new(376, 22), GraphicsSize::new(fill as u32, 9))
                    .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                    .draw(&mut display)
                    .ok();
            }
            write!(label, "{}%", percent.get()).ok();
        }
    }
    Text::with_baseline(
        label.as_str(),
        Point::new(410, 22),
        chrome_style(),
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();
    if battery.usb().is_connected() {
        Text::with_baseline("U", Point::new(386, 21), chrome_style(), Baseline::Top)
            .draw(&mut display)
            .ok();
    }
}

pub(crate) struct FrameTarget<'target, 'bytes> {
    image: &'target mut MonochromeImage<'bytes>,
}

impl<'target, 'bytes> FrameTarget<'target, 'bytes> {
    pub(crate) fn new(image: &'target mut MonochromeImage<'bytes>) -> Self {
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
    type Color = BinaryColor;
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
                self.image.set_pixel(x, y, color == BinaryColor::On);
            }
        }
        Ok(())
    }
}

pub(crate) struct FixedText<const CAPACITY: usize> {
    bytes: [u8; CAPACITY],
    length: usize,
}

impl<const CAPACITY: usize> FixedText<CAPACITY> {
    pub(crate) const fn new() -> Self {
        Self {
            bytes: [0; CAPACITY],
            length: 0,
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.length]).expect("fixed text only stores UTF-8")
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
