use embedded_graphics::{Drawable, geometry::Point};

use crate::{
    image::{PackedBitmap, PackedImage, Size},
    power::BatteryStatus,
    ui::{AppBar, CHROME_INK, FrameTarget, Icon, Label, TextRole, ui},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SleepView<'a> {
    Custom(PackedBitmap<'a>),
    BookCover(PackedBitmap<'a>),
    BuiltIn {
        status: &'a str,
        battery: BatteryStatus,
    },
}

impl<'a> SleepView<'a> {
    pub const fn custom(image: PackedBitmap<'a>) -> Self {
        Self::Custom(image)
    }

    pub const fn book_cover(cover: PackedBitmap<'a>) -> Self {
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
    target: &mut PackedImage<'_>,
) -> Result<(), SleepRenderError> {
    let frame_size = Size::new(480, 800).unwrap();
    if target.size() != frame_size {
        return Err(SleepRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }

    match view {
        SleepView::Custom(image) => copy_frame(image, target, |actual| {
            SleepRenderError::ImageSizeMismatch { actual }
        })?,
        SleepView::BookCover(image) => copy_frame(image, target, |actual| {
            SleepRenderError::CoverSizeMismatch { actual }
        })?,
        SleepView::BuiltIn { status, battery } => {
            target.clear_white();
            let mut display = FrameTarget::new(target);
            Icon::Moon
                .draw(&mut display, Point::new(228, 320), CHROME_INK)
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

fn copy_frame(
    source: PackedBitmap<'_>,
    target: &mut PackedImage<'_>,
    wrong_size: impl FnOnce(Size) -> SleepRenderError,
) -> Result<(), SleepRenderError> {
    if source.size() != target.size() {
        return Err(wrong_size(source.size()));
    }
    for y in 0..target.size().height() {
        for x in 0..target.size().width() {
            target.set_luma(x, y, source.luma(x, y));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{SleepRenderError, SleepView, render_sleep};
    use crate::{
        image::{PackedBitmap, PackedImage, READER_DEPTH, Size},
        power::BatteryStatus,
    };
    use std::{vec, vec::Vec};

    fn four_tone_frame() -> Vec<u8> {
        let size = Size::new(480, 800).unwrap();
        let mut bytes = vec![0xff; READER_DEPTH.byte_len(size).unwrap()];
        let mut frame = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
        for y in 0..800 {
            for x in 0..480 {
                frame.set_luma(x, y, [0, 85, 170, 255][x / 120]);
            }
        }
        bytes
    }

    #[test]
    fn cover_frame_preserves_every_source_tone_without_chrome() {
        let size = Size::new(480, 800).unwrap();
        let source_bytes = four_tone_frame();
        let source = PackedBitmap::new(size, READER_DEPTH, &source_bytes).unwrap();
        let mut bytes = vec![0; READER_DEPTH.byte_len(size).unwrap()];
        let mut frame = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();

        render_sleep(SleepView::book_cover(source), &mut frame).unwrap();

        for x in [0, 119, 120, 239, 240, 359, 360, 479] {
            assert_eq!(frame.luma(x, 400), [0, 85, 170, 255][x / 120]);
        }
    }

    #[test]
    fn custom_frame_preserves_every_source_tone() {
        let size = Size::new(480, 800).unwrap();
        let source_bytes = four_tone_frame();
        let source = PackedBitmap::new(size, READER_DEPTH, &source_bytes).unwrap();
        let mut bytes = vec![0; READER_DEPTH.byte_len(size).unwrap()];
        let mut frame = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();

        render_sleep(SleepView::custom(source), &mut frame).unwrap();

        assert_eq!(
            [30, 150, 270, 390].map(|x| frame.luma(x, 400)),
            [0, 85, 170, 255]
        );
    }

    #[test]
    fn renders_builtin_fallback() {
        let size = Size::new(480, 800).unwrap();
        let mut bytes = vec![0xff; READER_DEPTH.byte_len(size).unwrap()];
        let mut frame = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();

        render_sleep(
            SleepView::built_in("Position saved", BatteryStatus::default()),
            &mut frame,
        )
        .unwrap();

        assert!(bytes.iter().any(|byte| *byte != 0xff));
    }

    #[test]
    fn image_views_require_an_exact_frame() {
        let image_bytes = vec![0xaa; 176 * 264 / 8];
        let image = PackedBitmap::monochrome(Size::new(176, 264).unwrap(), &image_bytes).unwrap();
        let size = Size::new(480, 800).unwrap();
        let mut bytes = vec![0xff; READER_DEPTH.byte_len(size).unwrap()];
        let mut frame = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();

        assert_eq!(
            render_sleep(SleepView::custom(image), &mut frame),
            Err(SleepRenderError::ImageSizeMismatch {
                actual: Size::new(176, 264).unwrap()
            })
        );
        assert_eq!(
            render_sleep(SleepView::book_cover(image), &mut frame),
            Err(SleepRenderError::CoverSizeMismatch {
                actual: Size::new(176, 264).unwrap()
            })
        );
    }
}
