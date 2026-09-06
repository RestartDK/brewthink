use core::fmt::Write;

use embedded_graphics::{
    Drawable, Pixel,
    geometry::{Point, Size as GraphicsSize},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Primitive},
    primitives::{PrimitiveStyle, Rectangle},
};
use embedded_layout::View;

use crate::{
    app::LibraryState,
    image::{MonochromeBitmap, MonochromeImage, Size},
    power::BatteryStatus,
    ui::{AppBar, CommandBar, FixedText, FrameTarget, Label, Selection, TextRole, ui},
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
    battery: BatteryStatus,
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
    for index in state.visible_range() {
        if let Some(cover) = books[index].cover {
            cover_scale(cover)?;
        }
    }

    target.clear_white();
    let mut section = FixedText::<48>::new();
    write!(
        section,
        "Books  {} item{}",
        state.book_count(),
        if state.book_count() == 1 { "" } else { "s" }
    )
    .ok();
    let mut display = FrameTarget::new(target);
    if books.is_empty() {
        ui!(
            AppBar::new(section.as_str(), battery),
            Label::new("No books yet", TextRole::Heading).at(Point::new(120, 342)),
            Label::new("Add DRM-free EPUB files to /books", TextRole::Body)
                .at(Point::new(120, 382)),
            CommandBar::new(["Home", "", "", ""]),
        )
        .draw(&mut display)
        .ok();
        return Ok(());
    }

    let page_start = state.visible_range().start;
    let selected = state
        .selected()
        .expect("a non-empty library always has a selected book");
    ui!(
        AppBar::new(section.as_str(), battery),
        ShelfGrid::new(state, books),
        ShelfFooter::new(books[selected.index()], state, page_start),
    )
    .draw(&mut display)
    .ok();
    Ok(())
}

#[derive(Clone, Copy)]
struct ShelfGrid<'a> {
    state: LibraryState,
    books: &'a [ShelfBook<'a>],
    origin: Point,
}

impl<'a> ShelfGrid<'a> {
    const fn new(state: LibraryState, books: &'a [ShelfBook<'a>]) -> Self {
        Self {
            state,
            books,
            origin: Point::zero(),
        }
    }
}

impl View for ShelfGrid<'_> {
    fn translate_impl(&mut self, by: Point) {
        self.origin += by;
    }

    fn bounds(&self) -> Rectangle {
        Rectangle::new(
            self.origin + Point::new(25, 68),
            GraphicsSize::new(438, 565),
        )
    }
}

impl Drawable for ShelfGrid<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        for (visible_index, book_index) in self.state.visible_range().enumerate() {
            let tile = CoverTile::new(
                self.origin
                    + Point::new(
                        COVER_LEFT[visible_index % 2] as i32,
                        COVER_TOP[visible_index / 2] as i32,
                    ),
                self.books[book_index].cover,
                Selection::from_selected(
                    self.state
                        .selected()
                        .is_some_and(|selected| selected.index() == book_index),
                ),
            );
            tile.draw(target)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct CoverTile<'a> {
    top_left: Point,
    cover: Option<MonochromeBitmap<'a>>,
    selection: Selection,
}

impl<'a> CoverTile<'a> {
    const fn new(
        top_left: Point,
        cover: Option<MonochromeBitmap<'a>>,
        selection: Selection,
    ) -> Self {
        Self {
            top_left,
            cover,
            selection,
        }
    }
}

impl View for CoverTile<'_> {
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

    fn bounds(&self) -> Rectangle {
        Rectangle::new(
            self.top_left - Point::new(7, 7),
            GraphicsSize::new((COVER_WIDTH + 14) as u32, (COVER_HEIGHT + 14) as u32),
        )
    }
}

impl Drawable for CoverTile<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        match self.cover {
            Some(cover) => {
                let scale = if cover.size().width() == COVER_WIDTH {
                    1
                } else {
                    2
                };
                target.draw_iter((0..COVER_HEIGHT).flat_map(|y| {
                    (0..COVER_WIDTH).map(move |x| {
                        Pixel(
                            self.top_left + Point::new(x as i32, y as i32),
                            if cover.pixel_is_black(x / scale, y / scale) {
                                BinaryColor::On
                            } else {
                                BinaryColor::Off
                            },
                        )
                    })
                }))?;
            }
            None => {
                Rectangle::new(
                    self.top_left,
                    GraphicsSize::new(COVER_WIDTH as u32, COVER_HEIGHT as u32),
                )
                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
                .draw(target)?;
                Label::new("No cover", TextRole::Metadata)
                    .at(self.top_left + Point::new(62, 127))
                    .draw(target)?;
            }
        }
        self.bounds()
            .into_styled(PrimitiveStyle::with_stroke(
                BinaryColor::On,
                self.selection.stroke(1, 4),
            ))
            .draw(target)
            .map(|_| ())
    }
}

#[derive(Clone, Copy)]
struct ShelfFooter<'a> {
    book: ShelfBook<'a>,
    state: LibraryState,
    page_start: usize,
    top_left: Point,
}

impl<'a> ShelfFooter<'a> {
    const fn new(book: ShelfBook<'a>, state: LibraryState, page_start: usize) -> Self {
        Self {
            book,
            state,
            page_start,
            top_left: Point::new(18, 650),
        }
    }
}

impl View for ShelfFooter<'_> {
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

    fn bounds(&self) -> Rectangle {
        Rectangle::new(self.top_left, GraphicsSize::new(444, 128))
    }
}

impl Drawable for ShelfFooter<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        Rectangle::new(self.top_left, GraphicsSize::new(444, 2))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(target)?;
        let (first_line, second_line) = split_title(self.book.title, 48);
        Label::new(first_line, TextRole::Heading)
            .at(self.top_left + Point::new(0, 20))
            .clipped_to(GraphicsSize::new(444, 42))
            .draw(target)?;
        if let Some(second_line) = second_line {
            Label::new(second_line, TextRole::Heading)
                .at(self.top_left + Point::new(0, 41))
                .clipped_to(GraphicsSize::new(444, 21))
                .draw(target)?;
        }
        Label::new(self.book.creator, TextRole::Metadata)
            .at(self.top_left + Point::new(0, 73))
            .clipped_to(GraphicsSize::new(340, 12))
            .draw(target)?;

        let mut page = FixedText::<48>::new();
        let page_end = (self.page_start + 4).min(self.state.book_count());
        write!(
            page,
            "{}-{} / {}",
            self.page_start + 1,
            page_end,
            self.state.book_count()
        )
        .ok();
        Label::new(page.as_str(), TextRole::Metadata)
            .at(self.top_left + Point::new(376, 73))
            .draw(target)?;
        CommandBar::new(["Home", "Read", "Left", "Right"]).draw(target)
    }
}

fn cover_scale(cover: MonochromeBitmap<'_>) -> Result<usize, ShelfRenderError> {
    let source = cover.size();
    let full = Size::new(COVER_WIDTH, COVER_HEIGHT).unwrap();
    if source == full {
        return Ok(1);
    }
    if source.width().checked_mul(2) == Some(COVER_WIDTH)
        && source.height().checked_mul(2) == Some(COVER_HEIGHT)
    {
        return Ok(2);
    }
    Err(ShelfRenderError::CoverSizeMismatch { actual: source })
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

        render_shelf(
            LibraryState::new(books.len()),
            &books,
            crate::power::BatteryStatus::default(),
            &mut frame,
        )
        .unwrap();

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

        render_shelf(
            LibraryState::new(books.len()),
            &books,
            crate::power::BatteryStatus::default(),
            &mut frame,
        )
        .unwrap();

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

        render_shelf(
            state,
            &books,
            crate::power::BatteryStatus::default(),
            &mut frame,
        )
        .unwrap();

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
            render_shelf(
                LibraryState::new(1),
                &[],
                crate::power::BatteryStatus::default(),
                &mut frame,
            ),
            Err(ShelfRenderError::CatalogLengthMismatch { state: 1, books: 0 })
        );
    }
}
