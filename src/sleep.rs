use core::convert::Infallible;

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

use crate::image::{MonochromeBitmap, MonochromeImage, Size};

const FRAME_WIDTH: usize = 480;
const FRAME_HEIGHT: usize = 800;
const COVER_WIDTH: usize = 176;
const COVER_HEIGHT: usize = 264;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SleepView<'a> {
    title: &'a str,
    creator: &'a str,
    status: &'a str,
    cover: Option<MonochromeBitmap<'a>>,
}

impl<'a> SleepView<'a> {
    pub const fn new(
        title: &'a str,
        creator: &'a str,
        status: &'a str,
        cover: Option<MonochromeBitmap<'a>>,
    ) -> Self {
        Self {
            title,
            creator,
            status,
            cover,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SleepRenderError {
    WrongFrameSize { actual: Size },
    CoverSizeMismatch { actual: Size },
}

pub fn render_sleep(
    view: SleepView<'_>,
    target: &mut MonochromeImage<'_>,
) -> Result<(), SleepRenderError> {
    let expected =
        Size::new(FRAME_WIDTH, FRAME_HEIGHT).expect("sleep frame dimensions are non-zero");
    if target.size() != expected {
        return Err(SleepRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }
    if let Some(cover) = view.cover
        && cover.size() != Size::new(COVER_WIDTH, COVER_HEIGHT).unwrap()
    {
        return Err(SleepRenderError::CoverSizeMismatch {
            actual: cover.size(),
        });
    }

    target.clear_white();
    let mut display = FrameTarget::new(target);
    let small = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let heading = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::On);
    Text::with_baseline("BREWTHINK", Point::new(18, 20), heading, Baseline::Top)
        .draw(&mut display)
        .ok();
    Text::with_baseline("SLEEP", Point::new(426, 24), small, Baseline::Top)
        .draw(&mut display)
        .ok();
    Rectangle::new(Point::new(18, 52), GraphicsSize::new(444, 2))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut display)
        .ok();

    match view.cover {
        Some(cover) => {
            for y in 0..COVER_HEIGHT {
                for x in 0..COVER_WIDTH {
                    target.set_pixel(152 + x, 145 + y, cover.pixel_is_black(x, y));
                }
            }
        }
        None => {
            let mut display = FrameTarget::new(target);
            Rectangle::new(Point::new(152, 145), GraphicsSize::new(176, 264))
                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                .draw(&mut display)
                .ok();
            Text::with_baseline("NO COVER", Point::new(214, 272), small, Baseline::Top)
                .draw(&mut display)
                .ok();
        }
    }

    let mut display = FrameTarget::new(target);
    let title_clip = Rectangle::new(Point::new(32, 455), GraphicsSize::new(416, 42));
    Text::with_baseline(view.title, Point::new(32, 455), heading, Baseline::Top)
        .draw(&mut display.clipped(&title_clip))
        .ok();
    let creator_clip = Rectangle::new(Point::new(32, 510), GraphicsSize::new(416, 14));
    Text::with_baseline(view.creator, Point::new(32, 510), small, Baseline::Top)
        .draw(&mut display.clipped(&creator_clip))
        .ok();
    Text::with_baseline(view.status, Point::new(32, 660), small, Baseline::Top)
        .draw(&mut display)
        .ok();
    Rectangle::new(Point::new(32, 700), GraphicsSize::new(416, 1))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut display)
        .ok();
    Text::with_baseline(
        "PRESS POWER TO WAKE",
        Point::new(178, 724),
        small,
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();
    Ok(())
}

struct FrameTarget<'target, 'bytes> {
    image: &'target mut MonochromeImage<'bytes>,
}

impl<'target, 'bytes> FrameTarget<'target, 'bytes> {
    fn new(image: &'target mut MonochromeImage<'bytes>) -> Self {
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
        for Pixel(point, color) in pixels {
            let (Ok(x), Ok(y)) = (usize::try_from(point.x), usize::try_from(point.y)) else {
                continue;
            };
            if x < self.image.size().width() && y < self.image.size().height() {
                self.image.set_pixel(x, y, color == BinaryColor::On);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec;

    use super::{SleepView, render_sleep};
    use crate::image::{MonochromeBitmap, MonochromeImage, Size};

    #[test]
    fn renders_a_retained_sleep_cover_frame() {
        let cover_bytes = vec![0xAA; 176 * 264 / 8];
        let cover = MonochromeBitmap::new(Size::new(176, 264).unwrap(), &cover_bytes).unwrap();
        let mut bytes = vec![0xFF; 48_000];
        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();

        render_sleep(
            SleepView::new("A Small Book", "An Author", "PAGE 3", Some(cover)),
            &mut frame,
        )
        .unwrap();

        assert!(bytes.iter().any(|byte| *byte != 0xFF));
    }
}
