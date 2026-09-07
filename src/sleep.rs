use embedded_graphics::{Drawable, geometry::Point, pixelcolor::BinaryColor};

use crate::{
    image::{MonochromeBitmap, MonochromeImage, Size},
    power::BatteryStatus,
    ui::{AppBar, FrameTarget, Icon, Label, TextRole, ui},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SleepView<'a> {
    Custom(MonochromeBitmap<'a>),
    BookCover(MonochromeBitmap<'a>),
    BuiltIn {
        status: &'a str,
        battery: BatteryStatus,
    },
}

impl<'a> SleepView<'a> {
    pub const fn custom(image: MonochromeBitmap<'a>) -> Self {
        Self::Custom(image)
    }
    pub const fn book_cover(cover: MonochromeBitmap<'a>) -> Self {
        Self::BookCover(cover)
    }
    pub const fn built_in(status: &'a str, battery: BatteryStatus) -> Self {
        Self::BuiltIn { status, battery }
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
    let frame_size = Size::new(480, 800).unwrap();
    if target.size() != frame_size {
        return Err(SleepRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }
    match view {
        SleepView::Custom(image) | SleepView::BookCover(image) if image.size() == frame_size => {
            for y in 0..800 {
                for x in 0..480 {
                    target.set_pixel(x, y, image.pixel_is_black(x, y));
                }
            }
        }
        SleepView::Custom(image) => {
            return Err(SleepRenderError::ImageSizeMismatch {
                actual: image.size(),
            });
        }
        SleepView::BookCover(cover) => {
            if cover.size() != Size::new(176, 264).unwrap() {
                return Err(SleepRenderError::CoverSizeMismatch {
                    actual: cover.size(),
                });
            }
            target.clear_white();
            for y in 0..720 {
                for x in 0..480 {
                    target.set_pixel(
                        x,
                        40 + y,
                        cover.pixel_is_black(x * 176 / 480, y * 264 / 720),
                    );
                }
            }
        }
        SleepView::BuiltIn { status, battery } => {
            target.clear_white();
            let mut display = FrameTarget::new(target);
            Icon::Moon
                .draw(&mut display, Point::new(228, 320), BinaryColor::On)
                .ok();
            ui!(
                AppBar::new("Sleep", battery),
                Label::new("Sleeping", TextRole::Heading).at(Point::new(204, 372)),
                Label::new(status, TextRole::Body).at(Point::new(32, 660)),
                Label::new("Press Power to wake", TextRole::CommandHint).at(Point::new(183, 744)),
            )
            .draw(&mut display)
            .ok();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::{SleepRenderError, SleepView, render_sleep};
    use crate::{
        image::{MonochromeBitmap, MonochromeImage, Size},
        power::BatteryStatus,
    };
    use std::vec;

    #[test]
    fn cover_frame_contains_only_the_scaled_cover_and_white_margins() {
        let cover_bytes = vec![0xAA; 176 * 264 / 8];
        let cover = MonochromeBitmap::new(Size::new(176, 264).unwrap(), &cover_bytes).unwrap();
        let mut bytes = vec![0xFF; 48_000];
        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();
        render_sleep(SleepView::book_cover(cover), &mut frame).unwrap();
        for y in 0..800 {
            for x in 0..480 {
                let expected = (40..760).contains(&y)
                    && cover.pixel_is_black(x * 176 / 480, (y - 40) * 264 / 720);
                assert_eq!(frame.pixel_is_black(x, y), expected, "pixel {x}, {y}");
            }
        }
    }

    #[test]
    fn renders_builtin_fallback() {
        let mut bytes = vec![0xFF; 48_000];
        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();
        render_sleep(
            SleepView::built_in("Position saved", BatteryStatus::default()),
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
            render_sleep(SleepView::custom(image), &mut frame),
            Err(SleepRenderError::ImageSizeMismatch {
                actual: Size::new(176, 264).unwrap()
            })
        );
    }
}
