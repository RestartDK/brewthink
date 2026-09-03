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

const POWER_SYMBOL_X: usize = 386;
const POWER_SYMBOL_Y: usize = 22;
const POWER_SYMBOL_ROWS: [u8; 9] = [
    0b00011, 0b00110, 0b01110, 0b11111, 0b00110, 0b01110, 0b01100, 0b11000, 0b10000,
];

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
            if fill > 0 && !battery.usb().is_connected() {
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
        draw_external_power_symbol(target);
    }
}

fn draw_external_power_symbol(target: &mut MonochromeImage<'_>) {
    for y in POWER_SYMBOL_Y..POWER_SYMBOL_Y + POWER_SYMBOL_ROWS.len() {
        for x in POWER_SYMBOL_X - 2..POWER_SYMBOL_X + 7 {
            target.set_pixel(x, y, false);
        }
    }
    for (row, pixels) in POWER_SYMBOL_ROWS.into_iter().enumerate() {
        for column in 0..5 {
            if pixels & (1 << (4 - column)) != 0 {
                target.set_pixel(POWER_SYMBOL_X + column, POWER_SYMBOL_Y + row, true);
            }
        }
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

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec;

    use super::draw_app_bar;
    use crate::{
        image::{MonochromeBitmap, MonochromeImage, Size},
        input::UsbState,
        power::BatteryStatus,
    };

    fn render_battery(percent: u8, usb: UsbState) -> std::vec::Vec<u8> {
        let mut bytes = vec![0xFF; 480 * 800 / 8];
        let mut image = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();
        draw_app_bar(
            &mut image,
            "HOME",
            BatteryStatus::from_percent(percent, usb),
        );
        bytes
    }

    #[test]
    fn external_power_hides_the_capacity_fill() {
        let bytes = render_battery(100, UsbState::Connected);
        let battery = MonochromeBitmap::new(Size::new(480, 800).unwrap(), &bytes).unwrap();

        for y in 22..31 {
            for x in (376..384).chain(393..399) {
                assert!(
                    !battery.pixel_is_black(x, y),
                    "battery fill remained at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn external_power_symbol_stays_inside_the_battery() {
        let size = Size::new(480, 800).unwrap();
        for percent in [0, 50, 75, 100] {
            let connected_bytes = render_battery(percent, UsbState::Connected);
            let disconnected_bytes = render_battery(percent, UsbState::Disconnected);
            let connected = MonochromeBitmap::new(size, &connected_bytes).unwrap();
            let disconnected = MonochromeBitmap::new(size, &disconnected_bytes).unwrap();
            let mut inside = 0;
            let mut outside = 0;

            for y in 0..800 {
                for x in 0..480 {
                    if connected.pixel_is_black(x, y) == disconnected.pixel_is_black(x, y) {
                        continue;
                    }
                    if (375..400).contains(&x) && (21..32).contains(&y) {
                        inside += 1;
                    } else {
                        outside += 1;
                    }
                }
            }

            assert!(inside > 0, "power symbol disappeared at {percent}%");
            assert_eq!(outside, 0, "power symbol escaped the battery at {percent}%");
        }
    }
}
