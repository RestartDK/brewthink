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

use crate::{
    image::{MonochromeBitmap, MonochromeImage, Size},
    power::BatteryStatus,
    ui::draw_app_bar,
};

const FRAME_WIDTH: usize = 480;
const FRAME_HEIGHT: usize = 800;
const COVER_WIDTH: usize = 176;
const COVER_HEIGHT: usize = 264;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustomSleepImageStatus {
    Missing,
    Ready,
    Invalid,
}

impl CustomSleepImageStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Missing => "NO IMAGE",
            Self::Ready => "IMAGE READY",
            Self::Invalid => "INVALID IMAGE",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SleepScreenContent<'a> {
    CustomImage(MonochromeBitmap<'a>),
    BookCover(MonochromeBitmap<'a>),
    BuiltIn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SleepView<'a> {
    title: &'a str,
    creator: &'a str,
    status: &'a str,
    content: SleepScreenContent<'a>,
    battery: BatteryStatus,
}

impl<'a> SleepView<'a> {
    pub const fn custom(image: MonochromeBitmap<'a>, battery: BatteryStatus) -> Self {
        Self {
            title: "",
            creator: "",
            status: "",
            content: SleepScreenContent::CustomImage(image),
            battery,
        }
    }

    pub const fn book_cover(
        title: &'a str,
        creator: &'a str,
        status: &'a str,
        cover: MonochromeBitmap<'a>,
        battery: BatteryStatus,
    ) -> Self {
        Self {
            title,
            creator,
            status,
            content: SleepScreenContent::BookCover(cover),
            battery,
        }
    }

    pub const fn built_in(status: &'a str, battery: BatteryStatus) -> Self {
        Self {
            title: "BREWTHINK",
            creator: "",
            status,
            content: SleepScreenContent::BuiltIn,
            battery,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SleepRenderError {
    WrongFrameSize { actual: Size },
    ImageSizeMismatch { actual: Size },
    CoverSizeMismatch { actual: Size },
}

pub fn render_sleep(
    view: SleepView<'_>,
    target: &mut MonochromeImage<'_>,
) -> Result<(), SleepRenderError> {
    let frame_size =
        Size::new(FRAME_WIDTH, FRAME_HEIGHT).expect("sleep frame dimensions are non-zero");
    if target.size() != frame_size {
        return Err(SleepRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }

    match view.content {
        SleepScreenContent::CustomImage(image) => {
            if image.size() != frame_size {
                return Err(SleepRenderError::ImageSizeMismatch {
                    actual: image.size(),
                });
            }
            for y in 0..FRAME_HEIGHT {
                for x in 0..FRAME_WIDTH {
                    target.set_pixel(x, y, image.pixel_is_black(x, y));
                }
            }
        }
        SleepScreenContent::BookCover(cover) => {
            let cover_size = Size::new(COVER_WIDTH, COVER_HEIGHT).unwrap();
            if cover.size() != cover_size {
                return Err(SleepRenderError::CoverSizeMismatch {
                    actual: cover.size(),
                });
            }
            render_composed(view, Some(cover), target);
        }
        SleepScreenContent::BuiltIn => render_composed(view, None, target),
    }
    Ok(())
}

fn render_composed(
    view: SleepView<'_>,
    cover: Option<MonochromeBitmap<'_>>,
    target: &mut MonochromeImage<'_>,
) {
    target.clear_white();
    draw_app_bar(target, "SLEEP", view.battery);
    let small = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let heading = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::On);

    match cover {
        Some(cover) => {
            for y in 0..COVER_HEIGHT {
                for x in 0..COVER_WIDTH {
                    target.set_pixel(152 + x, 145 + y, cover.pixel_is_black(x, y));
                }
            }
        }
        None => {
            let mut display = FrameTarget::new(target);
            Rectangle::new(Point::new(92, 190), GraphicsSize::new(296, 160))
                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 3))
                .draw(&mut display)
                .ok();
            Text::with_baseline("BREWTHINK", Point::new(178, 257), heading, Baseline::Top)
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

    use super::{SleepRenderError, SleepView, render_sleep};
    use crate::{
        image::{MonochromeBitmap, MonochromeImage, Size},
        power::BatteryStatus,
    };

    #[test]
    fn renders_book_cover_and_builtin_frames() {
        let cover_bytes = vec![0xAA; 176 * 264 / 8];
        let cover = MonochromeBitmap::new(Size::new(176, 264).unwrap(), &cover_bytes).unwrap();
        let mut bytes = vec![0xFF; 48_000];
        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();

        render_sleep(
            SleepView::book_cover(
                "A Small Book",
                "An Author",
                "PAGE 3",
                cover,
                BatteryStatus::default(),
            ),
            &mut frame,
        )
        .unwrap();
        assert!(bytes.iter().any(|byte| *byte != 0xFF));

        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();
        render_sleep(
            SleepView::built_in("HOME POSITION SAVED", BatteryStatus::default()),
            &mut frame,
        )
        .unwrap();
        assert!(bytes.iter().any(|byte| *byte != 0xFF));
    }

    #[test]
    fn custom_image_requires_an_exact_frame() {
        let image_bytes = vec![0xAA; 176 * 264 / 8];
        let image = MonochromeBitmap::new(Size::new(176, 264).unwrap(), &image_bytes).unwrap();
        let mut bytes = vec![0xFF; 48_000];
        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();

        assert_eq!(
            render_sleep(
                SleepView::custom(image, BatteryStatus::default()),
                &mut frame
            ),
            Err(SleepRenderError::ImageSizeMismatch {
                actual: Size::new(176, 264).unwrap()
            })
        );
    }
}
