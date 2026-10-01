use core::fmt::Write;

use embedded_graphics::{Drawable, geometry::Point};
use embedded_layout::{
    View,
    layout::linear::{FixedMargin, LinearLayout},
    view_group::Views,
};

use crate::{
    app::{LIBRARY_ROWS_PER_PAGE, LibraryRow, LibraryView},
    image::{PackedImage, Size},
    power::BatteryStatus,
    ui::{
        AppBar, BookListRow, CONTENT_LEFT, CommandBar, FixedText, FrameTarget, Icon, Label,
        Selection, TextRole, ui,
    },
};

const FRAME_WIDTH: usize = 480;
const FRAME_HEIGHT: usize = 800;
const ROW_SPACING: i32 = 14;
const ROWS_TOP: i32 = 86;
const COUNTER_TOP: i32 = 714;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryRenderError {
    WrongFrameSize { actual: Size },
}

pub fn render_library(
    view: LibraryView<'_>,
    battery: BatteryStatus,
    target: &mut PackedImage<'_>,
) -> Result<(), LibraryRenderError> {
    let expected =
        Size::new(FRAME_WIDTH, FRAME_HEIGHT).expect("the X4 frame has non-zero dimensions");
    if target.size() != expected {
        return Err(LibraryRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }
    target.clear_white();
    let mut display = FrameTarget::new(target);
    let heading = view.scope_name().unwrap_or("Books");
    if view.row_count() == 0 {
        ui!(
            AppBar::new(heading, battery),
            Label::new("No books yet", TextRole::Heading).at(Point::new(120, 342)),
            Label::new("Add DRM-free EPUB files to /books", TextRole::Body)
                .at(Point::new(120, 382)),
            CommandBar::new(["Home", "", "", ""]),
        )
        .draw(&mut display)
        .ok();
        return Ok(());
    }

    let mut counters: [FixedText<24>; LIBRARY_ROWS_PER_PAGE] =
        core::array::from_fn(|_| FixedText::new());
    for (slot, visible) in view.visible_rows().enumerate() {
        if let LibraryRow::Series { volumes, .. } = visible.row {
            write!(counters[slot], "{volumes} books").ok();
        }
    }
    let mut row_views =
        [BookListRow::new(Icon::Book, "", "", Selection::Idle); LIBRARY_ROWS_PER_PAGE];
    let mut row_count = 0;
    for (slot, visible) in view.visible_rows().enumerate() {
        let (icon, primary, secondary) = match visible.row {
            LibraryRow::Book { title, creator, .. } => (Icon::Book, title, creator),
            LibraryRow::Series { name, .. } => (Icon::Folder, name, counters[slot].as_str()),
        };
        row_views[slot] = BookListRow::new(
            icon,
            primary,
            secondary,
            Selection::from_selected(visible.selected),
        );
        row_count += 1;
    }
    let rows = LinearLayout::vertical(Views::new(&mut row_views[..row_count]))
        .with_spacing(FixedMargin(ROW_SPACING))
        .arrange()
        .translate(Point::new(CONTENT_LEFT, ROWS_TOP));

    let start = view.page() * LIBRARY_ROWS_PER_PAGE;
    let end = (start + LIBRARY_ROWS_PER_PAGE).min(view.row_count());
    let mut footer = FixedText::<64>::new();
    write!(footer, "{}-{} / {}", start + 1, end, view.row_count()).ok();
    let commands = if view.scope_name().is_some() {
        ["Back", "Open", "Previous", "Next"]
    } else {
        ["Home", "Open", "Previous", "Next"]
    };
    ui!(
        AppBar::new(heading, battery),
        rows,
        Label::new(footer.as_str(), TextRole::Metadata).at(Point::new(CONTENT_LEFT, COUNTER_TOP)),
        CommandBar::new(commands),
    )
    .draw(&mut display)
    .ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::{vec, vec::Vec};

    use crate::{
        app::{BookMeta, LibraryIndex, LibraryState, SeriesMeta, SeriesPosition},
        image::{PackedImage, READER_DEPTH, Size},
        input::UsbState,
        power::BatteryStatus,
    };

    use super::{LIBRARY_ROWS_PER_PAGE, LibraryRenderError, render_library};

    const WIDTH: usize = 480;
    const HEIGHT: usize = 800;

    fn index(books: &[BookMeta<'_>]) -> LibraryIndex {
        LibraryIndex::try_from(books).unwrap()
    }

    fn render(state: LibraryState, books: &[BookMeta<'_>]) -> Vec<u8> {
        let size = Size::new(WIDTH, HEIGHT).unwrap();
        let mut bytes = vec![0xFF; READER_DEPTH.byte_len(size).unwrap()];
        let view = state.view(books).unwrap();
        let mut image = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
        render_library(
            view,
            BatteryStatus::from_percent(82, UsbState::Disconnected),
            &mut image,
        )
        .unwrap();
        bytes
    }

    #[test]
    fn folder_and_book_rows_render_their_icons_and_selection() {
        let books = [
            BookMeta::new(
                "Volume Two",
                "An Author",
                Some(SeriesMeta::new(
                    "Collected Works",
                    Some(SeriesPosition::new(2)),
                )),
            ),
            BookMeta::new("Standalone", "Another Author", None),
            BookMeta::new(
                "Volume One",
                "An Author",
                Some(SeriesMeta::new(
                    "Collected Works",
                    Some(SeriesPosition::new(1)),
                )),
            ),
        ];
        let mut root = render(LibraryState::new(index(&books)), &books);
        {
            let image =
                PackedImage::new(Size::new(WIDTH, HEIGHT).unwrap(), READER_DEPTH, &mut root)
                    .unwrap();
            assert!(image.bitmap().is_monochrome());
            assert!(image.pixel_is_black(40, 109), "folder tab is missing");
            assert!(image.pixel_is_black(240, 86), "folder row lost its outline");
            for y in 86 + 31..86 + 33 {
                for x in 450..452 {
                    assert_eq!(
                        image.luma(x, y),
                        if x % 2 == 0 && y % 2 == 0 { 0 } else { 255 }
                    );
                }
            }
        }

        let mut state = LibraryState::new(index(&books));
        assert!(state.move_selection(crate::app::Direction::Down));
        let book = render(state, &books);
        assert_ne!(root, book);
    }

    #[test]
    fn the_second_page_renders_only_the_remaining_rows() {
        let titles: [BookMeta; 9] = [
            BookMeta::new("Book 1", "Author", None),
            BookMeta::new("Book 2", "Author", None),
            BookMeta::new("Book 3", "Author", None),
            BookMeta::new("Book 4", "Author", None),
            BookMeta::new("Book 5", "Author", None),
            BookMeta::new("Book 6", "Author", None),
            BookMeta::new("Book 7", "Author", None),
            BookMeta::new("Book 8", "Author", None),
            BookMeta::new("Book 9", "Author", None),
        ];
        assert_eq!(LIBRARY_ROWS_PER_PAGE, 8);
        let mut state = LibraryState::new(index(&titles));
        for _ in 0..8 {
            state.move_selection(crate::app::Direction::Down);
        }
        assert_eq!(state.page(), 1);
        let mut bytes = render(state, &titles);
        let image =
            PackedImage::new(Size::new(WIDTH, HEIGHT).unwrap(), READER_DEPTH, &mut bytes).unwrap();

        assert!(image.pixel_is_black(36, 115), "first row icon is missing");
        assert!(
            !image.pixel_is_black(34, 181),
            "an empty second row slot was drawn"
        );
    }

    #[test]
    fn renderer_rejects_a_wrong_frame_size() {
        let size = Size::new(240, 400).unwrap();
        let mut bytes = vec![0xFF; READER_DEPTH.byte_len(size).unwrap()];
        let mut image = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
        let books = [BookMeta::new("Book", "Author", None)];
        let state = LibraryState::new(index(&books));
        let view = state.view(&books).unwrap();

        assert_eq!(
            render_library(view, BatteryStatus::unknown(), &mut image),
            Err(LibraryRenderError::WrongFrameSize { actual: size })
        );
    }
}
