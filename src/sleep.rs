use embedded_graphics::{
    Drawable, Pixel,
    geometry::{Point, Size as GraphicsSize},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Primitive},
    primitives::{PrimitiveStyle, Rectangle},
};
use embedded_layout::View;

use crate::{
    image::{MonochromeBitmap, MonochromeImage, Size},
    power::BatteryStatus,
    ui::{AppBar, FrameTarget, Label, TextRole, ui},
};

const FRAME_WIDTH: usize = 480;
const FRAME_HEIGHT: usize = 800;
const COVER_WIDTH: usize = 176;
const COVER_HEIGHT: usize = 264;

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
            validate_cover(cover)?;
            render_composed(view, target);
        }
        SleepScreenContent::BuiltIn => render_composed(view, target),
    }
    Ok(())
}

fn render_composed(view: SleepView<'_>, target: &mut MonochromeImage<'_>) {
    target.clear_white();
    ui!(AppBar::new("SLEEP", view.battery), SleepContent::new(view),)
        .draw(&mut FrameTarget::new(target))
        .ok();
}

fn validate_cover(cover: MonochromeBitmap<'_>) -> Result<(), SleepRenderError> {
    let source = cover.size();
    if source == Size::new(COVER_WIDTH, COVER_HEIGHT).unwrap() {
        return Ok(());
    }
    Err(SleepRenderError::CoverSizeMismatch { actual: source })
}

#[derive(Clone, Copy)]
struct SleepContent<'a> {
    view: SleepView<'a>,
    origin: Point,
}

impl<'a> SleepContent<'a> {
    const fn new(view: SleepView<'a>) -> Self {
        Self {
            view,
            origin: Point::zero(),
        }
    }
}

impl View for SleepContent<'_> {
    fn translate_impl(&mut self, by: Point) {
        self.origin += by;
    }

    fn bounds(&self) -> Rectangle {
        Rectangle::new(
            self.origin + Point::new(32, 145),
            GraphicsSize::new(416, 597),
        )
    }
}

impl Drawable for SleepContent<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        let cover_top_left = self.origin + Point::new(152, 145);
        if let SleepScreenContent::BookCover(cover) = self.view.content {
            target.draw_iter((0..COVER_HEIGHT).flat_map(|y| {
                (0..COVER_WIDTH).map(move |x| {
                    Pixel(
                        cover_top_left + Point::new(x as i32, y as i32),
                        if cover.pixel_is_black(x, y) {
                            BinaryColor::On
                        } else {
                            BinaryColor::Off
                        },
                    )
                })
            }))?;
        } else {
            Rectangle::new(
                self.origin + Point::new(92, 190),
                GraphicsSize::new(296, 160),
            )
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 3))
            .draw(target)?;
            Label::new("BREWTHINK", TextRole::Heading)
                .at(self.origin + Point::new(178, 257))
                .draw(target)?;
        }

        Label::new(self.view.title, TextRole::Heading)
            .at(self.origin + Point::new(32, 455))
            .clipped_to(GraphicsSize::new(416, 42))
            .draw(target)?;
        Label::new(self.view.creator, TextRole::Body)
            .at(self.origin + Point::new(32, 510))
            .clipped_to(GraphicsSize::new(416, 14))
            .draw(target)?;
        Label::new(self.view.status, TextRole::Body)
            .at(self.origin + Point::new(32, 660))
            .draw(target)?;
        Rectangle::new(self.origin + Point::new(32, 700), GraphicsSize::new(416, 1))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(target)?;
        Label::new("PRESS POWER TO WAKE", TextRole::CommandHint)
            .at(self.origin + Point::new(178, 724))
            .draw(target)
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
