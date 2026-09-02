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
    app::LibraryState,
    image::{MonochromeBitmap, MonochromeImage, Region, Size},
};

const FRAME_WIDTH: usize = 480;
const FRAME_HEIGHT: usize = 800;
const COVER_WIDTH: usize = 176;
const COVER_HEIGHT: usize = 264;
const COVER_LEFT: [usize; 2] = [32, 272];
const COVER_TOP: [usize; 2] = [75, 362];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShelfBook<'a> {
    title: &'a str,
    creator: &'a str,
    cover: Option<MonochromeBitmap<'a>>,
}

impl<'a> ShelfBook<'a> {
    pub const fn new(
        title: &'a str,
        creator: &'a str,
        cover: Option<MonochromeBitmap<'a>>,
    ) -> Self {
        Self {
            title,
            creator,
            cover,
        }
    }

    pub const fn title(self) -> &'a str {
        self.title
    }

    pub const fn creator(self) -> &'a str {
        self.creator
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShelfRenderError {
    WrongFrameSize { actual: Size },
    CatalogLengthMismatch { state: usize, books: usize },
    CoverSizeMismatch { actual: Size },
}

pub fn render_shelf(
    state: LibraryState,
    books: &[ShelfBook<'_>],
    target: &mut MonochromeImage<'_>,
) -> Result<(), ShelfRenderError> {
    let expected =
        Size::new(FRAME_WIDTH, FRAME_HEIGHT).expect("the X4 frame has non-zero dimensions");
    if target.size() != expected {
        return Err(ShelfRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }
    if state.book_count() != books.len() {
        return Err(ShelfRenderError::CatalogLengthMismatch {
            state: state.book_count(),
            books: books.len(),
        });
    }

    target.clear_white();
    draw_header(target, state);

    if books.is_empty() {
        draw_empty_state(target);
        return Ok(());
    }

    let page_start = state.visible_range().start;
    for (visible_index, book_index) in state.visible_range().enumerate() {
        let book = books[book_index];
        let column = visible_index % 2;
        let row = visible_index / 2;
        let region = Region::new(
            COVER_LEFT[column],
            COVER_TOP[row],
            Size::new(COVER_WIDTH, COVER_HEIGHT).expect("cover dimensions are non-zero"),
        );
        match book.cover {
            Some(cover) => blit_cover(target, region, cover)?,
            None => draw_cover_placeholder(target, region),
        }
        draw_cover_frame(
            target,
            region,
            state.selected().is_some_and(|id| id.index() == book_index),
        );
    }

    let selected = state
        .selected()
        .expect("a non-empty library always has a selected book");
    draw_footer(target, books[selected.index()], state, page_start);
    Ok(())
}

pub fn render_shelf_cover(
    state: LibraryState,
    book_index: usize,
    cover: MonochromeBitmap<'_>,
    target: &mut MonochromeImage<'_>,
) -> Result<(), ShelfRenderError> {
    if target.size() != Size::new(FRAME_WIDTH, FRAME_HEIGHT).unwrap() {
        return Err(ShelfRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }
    let range = state.visible_range();
    if !range.contains(&book_index) {
        return Ok(());
    }
    let visible_index = book_index - range.start;
    let region = Region::new(
        COVER_LEFT[visible_index % 2],
        COVER_TOP[visible_index / 2],
        Size::new(COVER_WIDTH, COVER_HEIGHT).unwrap(),
    );
    blit_cover(target, region, cover)?;
    draw_cover_frame(
        target,
        region,
        state.selected().is_some_and(|id| id.index() == book_index),
    );
    Ok(())
}

fn draw_header(target: &mut MonochromeImage<'_>, state: LibraryState) {
    let mut display = FrameTarget::new(target);
    let heading = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::On);
    Text::with_baseline("BREWTHINK", Point::new(18, 18), heading, Baseline::Top)
        .draw(&mut display)
        .ok();

    let mut count = FixedText::<48>::new();
    write!(
        count,
        "LIBRARY  {} BOOK{}",
        state.book_count(),
        if state.book_count() == 1 { "" } else { "S" }
    )
    .ok();
    let label = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    Text::with_baseline(count.as_str(), Point::new(308, 24), label, Baseline::Top)
        .draw(&mut display)
        .ok();
    Rectangle::new(Point::new(18, 52), GraphicsSize::new(444, 2))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut display)
        .ok();
}

fn draw_empty_state(target: &mut MonochromeImage<'_>) {
    let mut display = FrameTarget::new(target);
    let heading = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::On);
    let body = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    Text::with_baseline(
        "NO BOOKS FOUND",
        Point::new(166, 342),
        heading,
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();
    Text::with_baseline(
        "Add DRM-free EPUB files to /Books",
        Point::new(135, 382),
        body,
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();
}

fn blit_cover(
    target: &mut MonochromeImage<'_>,
    region: Region,
    cover: MonochromeBitmap<'_>,
) -> Result<(), ShelfRenderError> {
    let source = cover.size();
    let target_size = region.size();
    let scale = if source == target_size {
        1
    } else if source.width().checked_mul(2) == Some(target_size.width())
        && source.height().checked_mul(2) == Some(target_size.height())
    {
        2
    } else {
        return Err(ShelfRenderError::CoverSizeMismatch { actual: source });
    };
    for y in 0..target_size.height() {
        for x in 0..target_size.width() {
            target.set_pixel(
                region.x() + x,
                region.y() + y,
                cover.pixel_is_black(x / scale, y / scale),
            );
        }
    }
    Ok(())
}

fn draw_cover_placeholder(target: &mut MonochromeImage<'_>, region: Region) {
    let mut display = FrameTarget::new(target);
    let rectangle = region_rectangle(region);
    rectangle
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut display)
        .ok();
    let label = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    Text::with_baseline(
        "NO COVER",
        Point::new(region.x() as i32 + 62, region.y() as i32 + 127),
        label,
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();
}

fn draw_cover_frame(target: &mut MonochromeImage<'_>, region: Region, selected: bool) {
    let mut display = FrameTarget::new(target);
    let width = if selected { 4 } else { 1 };
    Rectangle::new(
        Point::new(region.x() as i32 - 7, region.y() as i32 - 7),
        GraphicsSize::new(
            (region.size().width() + 14) as u32,
            (region.size().height() + 14) as u32,
        ),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, width))
    .draw(&mut display)
    .ok();
}

fn draw_footer(
    target: &mut MonochromeImage<'_>,
    book: ShelfBook<'_>,
    state: LibraryState,
    page_start: usize,
) {
    let mut display = FrameTarget::new(target);
    Rectangle::new(Point::new(18, 650), GraphicsSize::new(444, 2))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut display)
        .ok();

    let title = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::On);
    let title_clip = Rectangle::new(Point::new(18, 670), GraphicsSize::new(444, 42));
    let (first_line, second_line) = split_title(book.title, 48);
    Text::with_baseline(first_line, Point::new(18, 670), title, Baseline::Top)
        .draw(&mut display.clipped(&title_clip))
        .ok();
    if let Some(second_line) = second_line {
        Text::with_baseline(second_line, Point::new(18, 691), title, Baseline::Top)
            .draw(&mut display.clipped(&title_clip))
            .ok();
    }

    let body = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let creator_clip = Rectangle::new(Point::new(18, 723), GraphicsSize::new(340, 12));
    Text::with_baseline(book.creator, Point::new(18, 723), body, Baseline::Top)
        .draw(&mut display.clipped(&creator_clip))
        .ok();

    let mut page = FixedText::<48>::new();
    let page_end = (page_start + 4).min(state.book_count());
    write!(
        page,
        "{}-{} / {}",
        page_start + 1,
        page_end,
        state.book_count()
    )
    .ok();
    Text::with_baseline(page.as_str(), Point::new(394, 723), body, Baseline::Top)
        .draw(&mut display)
        .ok();
    Text::with_baseline(
        "ARROWS  MOVE     CONFIRM  OPEN",
        Point::new(18, 768),
        body,
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();
}

fn split_title(title: &str, line_length: usize) -> (&str, Option<&str>) {
    let Some(cutoff) = title
        .char_indices()
        .nth(line_length)
        .map(|(index, _)| index)
    else {
        return (title, None);
    };
    let first = &title[..cutoff];
    let split = first.rfind(char::is_whitespace).unwrap_or(cutoff);
    let remainder = title[split..].trim_start();
    (
        &title[..split],
        (!remainder.is_empty()).then_some(remainder),
    )
}

fn region_rectangle(region: Region) -> Rectangle {
    Rectangle::new(
        Point::new(region.x() as i32, region.y() as i32),
        GraphicsSize::new(region.size().width() as u32, region.size().height() as u32),
    )
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
            let Ok(x) = usize::try_from(point.x) else {
                continue;
            };
            let Ok(y) = usize::try_from(point.y) else {
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

    use crate::{
        app::{Direction, LibraryState},
        image::{MonochromeBitmap, MonochromeImage, Size},
    };

    use super::{ShelfBook, ShelfRenderError, render_shelf, split_title};

    const FRAME_SIZE: usize = 480 * 800 / 8;
    const COVER_SIZE: usize = 176 * 264 / 8;

    #[test]
    fn shelf_renders_four_covers_and_selected_metadata() {
        let black = vec![0; COVER_SIZE];
        let white = vec![0xFF; COVER_SIZE];
        let cover_size = Size::new(176, 264).unwrap();
        let black_cover = MonochromeBitmap::new(cover_size, &black).unwrap();
        let white_cover = MonochromeBitmap::new(cover_size, &white).unwrap();
        let books = [
            ShelfBook::new("Selected title", "First author", Some(black_cover)),
            ShelfBook::new("Second", "Second author", Some(white_cover)),
            ShelfBook::new("Third", "Third author", None),
            ShelfBook::new("Fourth", "Fourth author", Some(black_cover)),
        ];
        let mut bytes = vec![0; FRAME_SIZE];
        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();

        render_shelf(LibraryState::new(books.len()), &books, &mut frame).unwrap();

        assert!(frame.pixel_is_black(32, 75));
        assert!(!frame.pixel_is_black(272, 75));
        assert!(frame.pixel_is_black(25, 68));
        assert!(frame.pixel_is_black(18, 650));
        assert!(
            (670..712)
                .flat_map(|y| (18..250).map(move |x| (x, y)))
                .any(|(x, y)| frame.pixel_is_black(x, y))
        );
    }

    #[test]
    fn shelf_upscales_half_size_device_covers() {
        let black = vec![0; 88 * 132 / 8];
        let cover = MonochromeBitmap::new(Size::new(88, 132).unwrap(), &black).unwrap();
        let books = [ShelfBook::new("Book", "Author", Some(cover))];
        let mut bytes = vec![0; FRAME_SIZE];
        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();

        render_shelf(LibraryState::new(books.len()), &books, &mut frame).unwrap();

        assert!(frame.pixel_is_black(32, 75));
        assert!(frame.pixel_is_black(207, 338));
    }

    #[test]
    fn shelf_uses_the_page_containing_the_selection() {
        let black = vec![0; COVER_SIZE];
        let black_cover = MonochromeBitmap::new(Size::new(176, 264).unwrap(), &black).unwrap();
        let books = [ShelfBook::new("Book", "Author", Some(black_cover)); 5];
        let mut state = LibraryState::new(books.len());
        state.move_selection(Direction::Down);
        state.move_selection(Direction::Down);
        let mut bytes = vec![0; FRAME_SIZE];
        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();

        render_shelf(state, &books, &mut frame).unwrap();

        assert!(frame.pixel_is_black(32, 75));
        assert!(!frame.pixel_is_black(272, 200));
    }

    #[test]
    fn long_titles_wrap_at_a_word_boundary() {
        assert_eq!(
            split_title(
                "The Art of Doing Science and Engineering: Learning to Learn",
                48,
            ),
            (
                "The Art of Doing Science and Engineering:",
                Some("Learning to Learn"),
            )
        );
    }

    #[test]
    fn renderer_rejects_state_from_a_different_catalog() {
        let mut bytes = vec![0; FRAME_SIZE];
        let mut frame = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();

        assert_eq!(
            render_shelf(LibraryState::new(1), &[], &mut frame),
            Err(ShelfRenderError::CatalogLengthMismatch { state: 1, books: 0 })
        );
    }
}
