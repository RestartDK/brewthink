use core::fmt::Write;

use embedded_graphics::{
    Drawable,
    draw_target::DrawTargetExt,
    geometry::{Point, Size as GraphicsSize},
    mono_font::{
        MonoFont, MonoTextStyle,
        ascii::{FONT_4X6, FONT_6X9, FONT_6X12, FONT_7X13, FONT_7X14, FONT_9X18_BOLD, FONT_10X20},
    },
    pixelcolor::BinaryColor,
    prelude::DrawTarget,
    primitives::Rectangle,
    text::{Baseline, Text},
};

use crate::{
    app::{ReaderFont, ReaderFontSize, ReaderPreferences, ReaderSpacing, ReadingLocation},
    fonts::{
        BitmapFont,
        noto_serif::{
            NOTO_SERIF_12_BOLD, NOTO_SERIF_12_REGULAR, NOTO_SERIF_14_BOLD, NOTO_SERIF_14_REGULAR,
            NOTO_SERIF_16_BOLD, NOTO_SERIF_16_REGULAR,
        },
    },
    image::{MonochromeImage, Size},
    power::BatteryStatus,
    ui::{AppBar, CommandBar, FixedText, FrameTarget, Label, TextRole, ui},
};
use embedded_layout::View;

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
enum ReaderFace {
    Monospace(&'static MonoFont<'static>),
    Bitmap(&'static BitmapFont),
}

impl ReaderFace {
    fn character_width(self, character: char) -> usize {
        match self {
            Self::Monospace(font) => font.character_size.width as usize,
            Self::Bitmap(font) => font.character_width(character),
        }
    }

    fn text_width(self, text: &str) -> usize {
        match self {
            Self::Monospace(font) => text.chars().count() * font.character_size.width as usize,
            Self::Bitmap(font) => font.text_width(text),
        }
    }

    const fn line_height(self) -> usize {
        match self {
            Self::Monospace(font) => font.character_size.height as usize,
            Self::Bitmap(font) => font.line_height(),
        }
    }

    fn draw<D>(self, text: &str, position: Point, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = BinaryColor>,
    {
        match self {
            Self::Monospace(font) => Text::with_baseline(
                text,
                position,
                MonoTextStyle::new(font, BinaryColor::On),
                Baseline::Top,
            )
            .draw(target)
            .map(|_| ()),
            Self::Bitmap(font) => font.draw(text, position, target),
        }
    }
}

#[derive(Clone, Copy)]
pub struct ReaderTheme {
    body: ReaderFace,
    heading: ReaderFace,
    spacing: ReaderSpacing,
}

impl ReaderTheme {
    pub const fn from_preferences(preferences: ReaderPreferences) -> Self {
        let (body, heading) = match (preferences.font(), preferences.size()) {
            (ReaderFont::NotoSerif, ReaderFontSize::Small) => (
                ReaderFace::Bitmap(&NOTO_SERIF_12_REGULAR),
                ReaderFace::Bitmap(&NOTO_SERIF_12_BOLD),
            ),
            (ReaderFont::NotoSerif, ReaderFontSize::Medium) => (
                ReaderFace::Bitmap(&NOTO_SERIF_14_REGULAR),
                ReaderFace::Bitmap(&NOTO_SERIF_14_BOLD),
            ),
            (ReaderFont::NotoSerif, ReaderFontSize::Large) => (
                ReaderFace::Bitmap(&NOTO_SERIF_16_REGULAR),
                ReaderFace::Bitmap(&NOTO_SERIF_16_BOLD),
            ),
            (ReaderFont::Compact, ReaderFontSize::Small) => (
                ReaderFace::Monospace(&FONT_4X6),
                ReaderFace::Monospace(&FONT_9X18_BOLD),
            ),
            (ReaderFont::Compact, ReaderFontSize::Medium) => (
                ReaderFace::Monospace(&FONT_6X9),
                ReaderFace::Monospace(&FONT_9X18_BOLD),
            ),
            (ReaderFont::Compact, ReaderFontSize::Large) => (
                ReaderFace::Monospace(&FONT_7X13),
                ReaderFace::Monospace(&FONT_9X18_BOLD),
            ),
            (ReaderFont::Mono, ReaderFontSize::Small) => (
                ReaderFace::Monospace(&FONT_6X12),
                ReaderFace::Monospace(&FONT_9X18_BOLD),
            ),
            (ReaderFont::Mono, ReaderFontSize::Medium) => (
                ReaderFace::Monospace(&FONT_7X14),
                ReaderFace::Monospace(&FONT_9X18_BOLD),
            ),
            (ReaderFont::Mono, ReaderFontSize::Large) => (
                ReaderFace::Monospace(&FONT_10X20),
                ReaderFace::Monospace(&FONT_9X18_BOLD),
            ),
        };
        Self {
            body,
            heading,
            spacing: preferences.spacing(),
        }
    }

    pub fn character_width(self, style: ReaderStyle, character: char) -> usize {
        self.face(style).character_width(character)
    }

    pub fn text_width(self, style: ReaderStyle, text: &str) -> usize {
        self.face(style).text_width(text)
    }

    pub const fn line_width(self, style: ReaderStyle) -> usize {
        BODY_WIDTH_PIXELS - (style.left() - 18)
    }

    pub const fn line_height(self, style: ReaderStyle) -> usize {
        let height = self.face(style).line_height();
        match (self.body, self.spacing) {
            (ReaderFace::Bitmap(_), ReaderSpacing::Compact) => height * 95 / 100,
            (ReaderFace::Bitmap(_), ReaderSpacing::Normal) => height,
            (ReaderFace::Bitmap(_), ReaderSpacing::Relaxed) => height * 110 / 100,
            (ReaderFace::Monospace(_), ReaderSpacing::Compact) => height + 1,
            (ReaderFace::Monospace(_), ReaderSpacing::Normal) => height + 3,
            (ReaderFace::Monospace(_), ReaderSpacing::Relaxed) => height + 6,
        }
    }

    pub fn draw_text<D>(
        self,
        text: &str,
        style: ReaderStyle,
        position: Point,
        target: &mut D,
    ) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = BinaryColor>,
    {
        self.face(style).draw(text, position, target)
    }

    const fn face(self, style: ReaderStyle) -> ReaderFace {
        match style {
            ReaderStyle::Heading => self.heading,
            ReaderStyle::Body
            | ReaderStyle::Quote
            | ReaderStyle::ListItem
            | ReaderStyle::Preformatted
            | ReaderStyle::Caption => self.body,
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

    let theme = ReaderTheme::from_preferences(view.preferences);
    let mut height = BODY_TOP;
    for line in view.lines {
        height += theme.line_height(line.style);
        if height > BODY_BOTTOM {
            return Err(ReaderRenderError::ContentExceedsPage);
        }
    }

    let mut progress = FixedText::<64>::new();
    write!(
        progress,
        "PAGE {} / {}     LEFT/RIGHT  TURN     BACK  LIBRARY",
        view.location.page_index() + 1,
        view.location.page_count()
    )
    .ok();
    target.clear_white();
    ui!(
        AppBar::new(view.book_title, view.battery),
        ReaderContent::new(view, theme),
        CommandBar::new(progress.as_str()),
    )
    .draw(&mut FrameTarget::new(target))
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
    ui!(
        AppBar::new(book_title, battery),
        Label::new("BOOK ERROR", TextRole::Error).at(Point::new(176, 280)),
        Label::new(book_title, TextRole::Body)
            .at(Point::new(48, 330))
            .clipped_to(GraphicsSize::new(384, 22)),
        Label::new(message, TextRole::Body)
            .at(Point::new(48, 380))
            .clipped_to(GraphicsSize::new(384, 48)),
        Label::new("PRESS BACK TO RETURN TO LIBRARY", TextRole::Body).at(Point::new(144, 500)),
    )
    .draw(&mut FrameTarget::new(target))
    .ok();
    Ok(())
}

#[derive(Clone, Copy)]
struct ReaderContent<'a> {
    view: ReaderView<'a>,
    theme: ReaderTheme,
    origin: Point,
}

impl<'a> ReaderContent<'a> {
    const fn new(view: ReaderView<'a>, theme: ReaderTheme) -> Self {
        Self {
            view,
            theme,
            origin: Point::zero(),
        }
    }
}

impl View for ReaderContent<'_> {
    fn translate_impl(&mut self, by: Point) {
        self.origin += by;
    }

    fn bounds(&self) -> Rectangle {
        Rectangle::new(
            self.origin + Point::new(18, 70),
            GraphicsSize::new(BODY_WIDTH_PIXELS as u32, (BODY_BOTTOM - 70) as u32),
        )
    }
}

impl Drawable for ReaderContent<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        let mut chapter = FixedText::<48>::new();
        write!(
            chapter,
            "CHAPTER {}/{}",
            self.view.location.spine_index() + 1,
            self.view.location.spine_count()
        )
        .ok();
        Label::new(chapter.as_str(), TextRole::Metadata)
            .at(self.origin + Point::new(378, 70))
            .draw(target)?;
        Label::new(self.view.chapter_title, TextRole::Metadata)
            .at(self.origin + Point::new(18, 70))
            .clipped_to(GraphicsSize::new(340, 15))
            .draw(target)?;

        let body_clip = Rectangle::new(
            self.origin + Point::new(18, BODY_TOP as i32),
            GraphicsSize::new(BODY_WIDTH_PIXELS as u32, (BODY_BOTTOM - BODY_TOP) as u32),
        );
        let mut y = BODY_TOP;
        for line in self.view.lines {
            self.theme.draw_text(
                line.text,
                line.style,
                self.origin + Point::new(line.style.left() as i32, y as i32),
                &mut target.clipped(&body_clip),
            )?;
            y += self.theme.line_height(line.style);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec;

    use super::{
        BODY_BOTTOM, BODY_TOP, ReaderLine, ReaderRenderError, ReaderStyle, ReaderTheme, ReaderView,
        render_reader,
    };
    use crate::{
        app::{App, AppEffect, AppInput, AppView, ReaderPreferences},
        image::{MonochromeImage, Size},
    };

    #[test]
    fn default_reader_uses_crosspoint_noto_serif_metrics() {
        let theme = ReaderTheme::from_preferences(ReaderPreferences::default());

        assert_eq!(theme.line_height(ReaderStyle::Body), 40);
        assert!(
            theme.text_width(ReaderStyle::Body, "iii") < theme.text_width(ReaderStyle::Body, "WWW")
        );
    }

    #[test]
    fn renders_a_reader_page_into_the_exact_x4_frame() {
        let mut app = App::new(1);
        app.input(AppInput::Confirm);
        app.input(AppInput::Confirm);
        assert_eq!(app.chapter_loaded(1, 2).unwrap(), AppEffect::Render);
        let AppView::Reader(session) = app.view() else {
            panic!("reader view expected");
        };
        let location = session.location();
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
                app.reader_preferences(),
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
        assert_eq!(app.chapter_loaded(1, 1).unwrap(), AppEffect::Render);
        let AppView::Reader(session) = app.view() else {
            panic!("reader view expected");
        };
        let location = session.location();
        let line = ReaderLine::new("line", ReaderStyle::Body);
        let theme = super::ReaderTheme::from_preferences(app.reader_preferences());
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
                    app.reader_preferences(),
                    app.battery(),
                ),
                &mut frame
            ),
            Err(ReaderRenderError::ContentExceedsPage)
        );
    }
}
