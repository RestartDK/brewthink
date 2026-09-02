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
    app::ReadingLocation,
    image::{MonochromeImage, Size},
};

pub const FRAME_WIDTH: usize = 480;
pub const FRAME_HEIGHT: usize = 800;
pub const BODY_WIDTH_PIXELS: usize = 444;
pub const BODY_TOP: usize = 76;
pub const BODY_BOTTOM: usize = 736;
pub const BODY_LINE_HEIGHT: usize = 13;
pub const HEADING_LINE_HEIGHT: usize = 23;
pub const BODY_CHARACTERS: usize = BODY_WIDTH_PIXELS / 6;
pub const HEADING_CHARACTERS: usize = BODY_WIDTH_PIXELS / 9;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReaderStyle {
    Body,
    Heading,
    Quote,
    ListItem,
    Preformatted,
    Caption,
}

impl ReaderStyle {
    pub const fn line_height(self) -> usize {
        match self {
            Self::Heading => HEADING_LINE_HEIGHT,
            Self::Body | Self::Quote | Self::ListItem | Self::Preformatted | Self::Caption => {
                BODY_LINE_HEIGHT
            }
        }
    }

    pub const fn characters_per_line(self) -> usize {
        match self {
            Self::Heading => HEADING_CHARACTERS,
            Self::Body | Self::Quote | Self::ListItem | Self::Preformatted | Self::Caption => {
                BODY_CHARACTERS
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReaderLine<'a> {
    text: &'a str,
    style: ReaderStyle,
}

impl<'a> ReaderLine<'a> {
    pub const fn new(text: &'a str, style: ReaderStyle) -> Self {
        Self { text, style }
    }

    pub const fn text(self) -> &'a str {
        self.text
    }

    pub const fn style(self) -> ReaderStyle {
        self.style
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReaderView<'a> {
    book_title: &'a str,
    chapter_title: &'a str,
    lines: &'a [ReaderLine<'a>],
    location: ReadingLocation,
}

impl<'a> ReaderView<'a> {
    pub const fn new(
        book_title: &'a str,
        chapter_title: &'a str,
        lines: &'a [ReaderLine<'a>],
        location: ReadingLocation,
    ) -> Self {
        Self {
            book_title,
            chapter_title,
            lines,
            location,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReaderRenderError {
    WrongFrameSize { actual: Size },
    ContentExceedsPage,
}

pub fn render_reader(
    view: ReaderView<'_>,
    target: &mut MonochromeImage<'_>,
) -> Result<(), ReaderRenderError> {
    let expected =
        Size::new(FRAME_WIDTH, FRAME_HEIGHT).expect("reader frame dimensions are non-zero");
    if target.size() != expected {
        return Err(ReaderRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }

    target.clear_white();
    let mut display = FrameTarget::new(target);
    let body = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let heading = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::On);
    let header_clip = Rectangle::new(Point::new(18, 20), GraphicsSize::new(360, 13));
    Text::with_baseline(view.book_title, Point::new(18, 20), body, Baseline::Top)
        .draw(&mut display.clipped(&header_clip))
        .ok();

    let mut chapter = FixedText::<48>::new();
    write!(
        chapter,
        "{}/{}",
        view.location.spine_index() + 1,
        view.location.spine_count()
    )
    .ok();
    Text::with_baseline(chapter.as_str(), Point::new(426, 20), body, Baseline::Top)
        .draw(&mut display)
        .ok();
    Rectangle::new(Point::new(18, 48), GraphicsSize::new(444, 2))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut display)
        .ok();

    let chapter_clip = Rectangle::new(Point::new(18, 57), GraphicsSize::new(444, 19));
    Text::with_baseline(view.chapter_title, Point::new(18, 57), body, Baseline::Top)
        .draw(&mut display.clipped(&chapter_clip))
        .ok();

    let body_clip = Rectangle::new(
        Point::new(18, BODY_TOP as i32),
        GraphicsSize::new(BODY_WIDTH_PIXELS as u32, (BODY_BOTTOM - BODY_TOP) as u32),
    );
    let mut y = BODY_TOP;
    for line in view.lines {
        let height = line.style.line_height();
        if y + height > BODY_BOTTOM {
            return Err(ReaderRenderError::ContentExceedsPage);
        }
        let (x, text_style) = match line.style {
            ReaderStyle::Heading => (18, heading),
            ReaderStyle::Quote => (36, body),
            ReaderStyle::ListItem => (30, body),
            ReaderStyle::Caption => (30, body),
            ReaderStyle::Body | ReaderStyle::Preformatted => (18, body),
        };
        Text::with_baseline(
            line.text,
            Point::new(x, y as i32),
            text_style,
            Baseline::Top,
        )
        .draw(&mut display.clipped(&body_clip))
        .ok();
        y += height;
    }

    Rectangle::new(Point::new(18, 748), GraphicsSize::new(444, 1))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut display)
        .ok();
    let mut progress = FixedText::<64>::new();
    write!(
        progress,
        "PAGE {} / {}     LEFT/RIGHT  TURN     BACK  LIBRARY",
        view.location.page_index() + 1,
        view.location.page_count()
    )
    .ok();
    Text::with_baseline(progress.as_str(), Point::new(18, 768), body, Baseline::Top)
        .draw(&mut display)
        .ok();
    Ok(())
}

pub fn render_reader_error(
    book_title: &str,
    message: &str,
    target: &mut MonochromeImage<'_>,
) -> Result<(), ReaderRenderError> {
    let expected =
        Size::new(FRAME_WIDTH, FRAME_HEIGHT).expect("reader frame dimensions are non-zero");
    if target.size() != expected {
        return Err(ReaderRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }
    target.clear_white();
    let mut display = FrameTarget::new(target);
    let body = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let heading = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::On);
    Text::with_baseline("BREWTHINK", Point::new(18, 20), heading, Baseline::Top)
        .draw(&mut display)
        .ok();
    Rectangle::new(Point::new(18, 52), GraphicsSize::new(444, 2))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut display)
        .ok();
    Text::with_baseline("BOOK ERROR", Point::new(176, 280), heading, Baseline::Top)
        .draw(&mut display)
        .ok();
    let title_clip = Rectangle::new(Point::new(48, 330), GraphicsSize::new(384, 22));
    Text::with_baseline(book_title, Point::new(48, 330), body, Baseline::Top)
        .draw(&mut display.clipped(&title_clip))
        .ok();
    let message_clip = Rectangle::new(Point::new(48, 380), GraphicsSize::new(384, 48));
    Text::with_baseline(message, Point::new(48, 380), body, Baseline::Top)
        .draw(&mut display.clipped(&message_clip))
        .ok();
    Text::with_baseline(
        "PRESS BACK TO RETURN TO LIBRARY",
        Point::new(144, 500),
        body,
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

struct FixedText<const CAPACITY: usize> {
    bytes: [u8; CAPACITY],
    length: usize,
}

impl<const CAPACITY: usize> FixedText<CAPACITY> {
    const fn new() -> Self {
        Self {
            bytes: [0; CAPACITY],
            length: 0,
        }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.length]).unwrap_or("")
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

    use super::{
        BODY_BOTTOM, BODY_TOP, ReaderLine, ReaderRenderError, ReaderStyle, ReaderView,
        render_reader,
    };
    use crate::{
        app::{App, AppEffect, AppInput},
        image::{MonochromeImage, Size},
    };

    #[test]
    fn renders_a_reader_page_into_the_exact_x4_frame() {
        let mut app = App::new(1);
        app.input(AppInput::Confirm);
        let AppEffect::RenderReader(location) = app.chapter_loaded(1, 2).unwrap() else {
            panic!("reader effect expected");
        };
        let lines = [
            ReaderLine::new("Chapter one", ReaderStyle::Heading),
            ReaderLine::new("Readable words survive reflow.", ReaderStyle::Body),
            ReaderLine::new("A quoted thought.", ReaderStyle::Quote),
        ];
        let mut bytes = vec![0xFF; 48_000];
        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();

        render_reader(
            ReaderView::new("A Small Book", "Chapter one", &lines, location),
            &mut frame,
        )
        .unwrap();

        assert!(bytes.iter().any(|byte| *byte != 0xFF));
        assert_eq!(bytes.len(), 48_000);
    }

    #[test]
    fn rejects_lines_that_exceed_the_bounded_body_region() {
        let mut app = App::new(1);
        app.input(AppInput::Confirm);
        let AppEffect::RenderReader(location) = app.chapter_loaded(1, 1).unwrap() else {
            panic!("reader effect expected");
        };
        let line = ReaderLine::new("line", ReaderStyle::Body);
        let lines = vec![line; (BODY_BOTTOM - BODY_TOP) / line.style().line_height() + 1];
        let mut bytes = vec![0xFF; 48_000];
        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();

        assert_eq!(
            render_reader(
                ReaderView::new("Book", "Chapter", &lines, location),
                &mut frame
            ),
            Err(ReaderRenderError::ContentExceedsPage)
        );
    }
}
