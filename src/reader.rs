use core::{convert::Infallible, fmt::Write};

use embedded_graphics::{
    Drawable, Pixel,
    draw_target::DrawTargetExt,
    geometry::{OriginDimensions, Point, Size as GraphicsSize},
    mono_font::{
        MonoFont, MonoTextStyle,
        ascii::{
            FONT_4X6, FONT_5X7, FONT_6X9, FONT_6X10, FONT_6X12, FONT_7X13, FONT_7X14, FONT_9X15,
            FONT_9X18_BOLD, FONT_10X20,
        },
    },
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Primitive},
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};

use crate::{
    app::{ReaderFont, ReaderFontSize, ReaderPreferences, ReaderSpacing, ReadingLocation},
    image::{MonochromeImage, Size},
    power::BatteryStatus,
    ui::draw_app_bar,
};

pub const FRAME_WIDTH: usize = 480;
pub const FRAME_HEIGHT: usize = 800;
pub const BODY_WIDTH_PIXELS: usize = 444;
pub const BODY_TOP: usize = 92;
pub const BODY_BOTTOM: usize = 736;

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
    pub const fn left(self) -> usize {
        match self {
            Self::Heading | Self::Body | Self::Preformatted => 18,
            Self::ListItem | Self::Caption => 30,
            Self::Quote => 36,
        }
    }
}

#[derive(Clone, Copy)]
pub struct ReaderTheme {
    body: &'static MonoFont<'static>,
    line_gap: usize,
}

impl ReaderTheme {
    pub const fn from_preferences(preferences: ReaderPreferences) -> Self {
        let body = match (preferences.font(), preferences.size()) {
            (ReaderFont::Book, ReaderFontSize::Small) => &FONT_5X7,
            (ReaderFont::Book, ReaderFontSize::Medium) => &FONT_6X10,
            (ReaderFont::Book, ReaderFontSize::Large) => &FONT_9X15,
            (ReaderFont::Compact, ReaderFontSize::Small) => &FONT_4X6,
            (ReaderFont::Compact, ReaderFontSize::Medium) => &FONT_6X9,
            (ReaderFont::Compact, ReaderFontSize::Large) => &FONT_7X13,
            (ReaderFont::Mono, ReaderFontSize::Small) => &FONT_6X12,
            (ReaderFont::Mono, ReaderFontSize::Medium) => &FONT_7X14,
            (ReaderFont::Mono, ReaderFontSize::Large) => &FONT_10X20,
        };
        let line_gap = match preferences.spacing() {
            ReaderSpacing::Compact => 1,
            ReaderSpacing::Normal => 3,
            ReaderSpacing::Relaxed => 6,
        };
        Self { body, line_gap }
    }

    pub const fn font(self, style: ReaderStyle) -> &'static MonoFont<'static> {
        match style {
            ReaderStyle::Heading => &FONT_9X18_BOLD,
            ReaderStyle::Body
            | ReaderStyle::Quote
            | ReaderStyle::ListItem
            | ReaderStyle::Preformatted
            | ReaderStyle::Caption => self.body,
        }
    }

    pub const fn line_height(self, style: ReaderStyle) -> usize {
        self.font(style).character_size.height as usize + self.line_gap
    }

    pub const fn characters_per_line(self, style: ReaderStyle) -> usize {
        let inset = style.left() - 18;
        (BODY_WIDTH_PIXELS - inset) / self.font(style).character_size.width as usize
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
    preferences: ReaderPreferences,
    battery: BatteryStatus,
}

impl<'a> ReaderView<'a> {
    pub const fn new(
        book_title: &'a str,
        chapter_title: &'a str,
        lines: &'a [ReaderLine<'a>],
        location: ReadingLocation,
        preferences: ReaderPreferences,
        battery: BatteryStatus,
    ) -> Self {
        Self {
            book_title,
            chapter_title,
            lines,
            location,
            preferences,
            battery,
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
    draw_app_bar(target, view.book_title, view.battery);
    let mut display = FrameTarget::new(target);
    let chrome = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let theme = ReaderTheme::from_preferences(view.preferences);

    let mut chapter = FixedText::<48>::new();
    write!(
        chapter,
        "CHAPTER {}/{}",
        view.location.spine_index() + 1,
        view.location.spine_count()
    )
    .ok();
    Text::with_baseline(chapter.as_str(), Point::new(378, 70), chrome, Baseline::Top)
        .draw(&mut display)
        .ok();

    let chapter_clip = Rectangle::new(Point::new(18, 70), GraphicsSize::new(340, 15));
    Text::with_baseline(
        view.chapter_title,
        Point::new(18, 70),
        chrome,
        Baseline::Top,
    )
    .draw(&mut display.clipped(&chapter_clip))
    .ok();

    let body_clip = Rectangle::new(
        Point::new(18, BODY_TOP as i32),
        GraphicsSize::new(BODY_WIDTH_PIXELS as u32, (BODY_BOTTOM - BODY_TOP) as u32),
    );
    let mut y = BODY_TOP;
    for line in view.lines {
        let height = theme.line_height(line.style);
        if y + height > BODY_BOTTOM {
            return Err(ReaderRenderError::ContentExceedsPage);
        }
        let text_style = MonoTextStyle::new(theme.font(line.style), BinaryColor::On);
        Text::with_baseline(
            line.text,
            Point::new(line.style.left() as i32, y as i32),
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
    Text::with_baseline(
        progress.as_str(),
        Point::new(18, 768),
        chrome,
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();
    Ok(())
}

pub fn render_reader_error(
    book_title: &str,
    message: &str,
    battery: BatteryStatus,
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
    draw_app_bar(target, book_title, battery);
    let mut display = FrameTarget::new(target);
    let body = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let heading = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::On);
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
            ReaderView::new(
                "A Small Book",
                "Chapter one",
                &lines,
                location,
                app.preferences(),
                app.battery(),
            ),
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
        app.input(AppInput::Confirm);
        let AppEffect::RenderReader(location) = app.chapter_loaded(1, 1).unwrap() else {
            panic!("reader effect expected");
        };
        let line = ReaderLine::new("line", ReaderStyle::Body);
        let theme = super::ReaderTheme::from_preferences(app.preferences());
        let lines = vec![line; (BODY_BOTTOM - BODY_TOP) / theme.line_height(line.style()) + 1];
        let mut bytes = vec![0xFF; 48_000];
        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();

        assert_eq!(
            render_reader(
                ReaderView::new(
                    "Book",
                    "Chapter",
                    &lines,
                    location,
                    app.preferences(),
                    app.battery(),
                ),
                &mut frame
            ),
            Err(ReaderRenderError::ContentExceedsPage)
        );
    }
}
