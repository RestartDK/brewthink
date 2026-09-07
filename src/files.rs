use core::fmt::Write;

use embedded_graphics::{Drawable, geometry::Point};
use embedded_layout::{
    View,
    layout::linear::{FixedMargin, LinearLayout},
    view_group::Views,
};

use crate::{
    app::FilesState,
    image::{PackedImage, Size},
    power::BatteryStatus,
    ui::{
        AppBar, CONTENT_LEFT, CommandBar, FRAME_HEIGHT, FRAME_WIDTH, FileRow, FixedText,
        FrameTarget, Label, Selection, TextRole, ui,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileItem<'a> {
    name: &'a str,
    size: u32,
    kind: FileKind,
}

impl<'a> FileItem<'a> {
    pub const fn new(name: &'a str, size: u32, kind: FileKind) -> Self {
        Self { name, size, kind }
    }

    pub const fn name(self) -> &'a str {
        self.name
    }

    pub const fn size(self) -> u32 {
        self.size
    }

    pub const fn kind(self) -> FileKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKind {
    Epub,
    Jpeg,
    Png,
}

impl FileKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Epub => "EPUB",
            Self::Jpeg => "JPEG image",
            Self::Png => "PNG image",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilesRenderError {
    WrongFrameSize { actual: Size },
    CatalogLengthMismatch { state: usize, files: usize },
}

pub fn render_files(
    state: FilesState,
    files: &[FileItem<'_>],
    battery: BatteryStatus,
    target: &mut PackedImage<'_>,
) -> Result<(), FilesRenderError> {
    let expected =
        Size::new(FRAME_WIDTH, FRAME_HEIGHT).expect("files frame dimensions are non-zero");
    if target.size() != expected {
        return Err(FilesRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }
    if state.file_count() != files.len() {
        return Err(FilesRenderError::CatalogLengthMismatch {
            state: state.file_count(),
            files: files.len(),
        });
    }

    target.clear_white();
    let mut display = FrameTarget::new(target);
    if files.is_empty() {
        ui!(
            AppBar::new("Files", battery),
            Label::new("No books or images", TextRole::Heading).at(Point::new(90, 326)),
            Label::new("Add EPUBs to /books or images to /files", TextRole::Body)
                .at(Point::new(90, 368)),
            CommandBar::new(["Home", "", "", ""]),
        )
        .draw(&mut display)
        .ok();
        return Ok(());
    }

    let range = state.visible_range();
    let mut row_views = [FileRow::new("", 0, FileKind::Epub, Selection::Idle); 8];
    for (row, index) in range.clone().enumerate() {
        row_views[row] = FileRow::new(
            files[index].name(),
            files[index].size(),
            files[index].kind(),
            Selection::from_selected(
                state
                    .selected()
                    .is_some_and(|selected| selected.index() == index),
            ),
        );
    }
    let rows = LinearLayout::vertical(Views::new(&mut row_views[..range.len()]))
        .with_spacing(FixedMargin(14))
        .arrange()
        .translate(Point::new(CONTENT_LEFT, 86));

    let mut footer = FixedText::<64>::new();
    write!(
        footer,
        "{}-{} / {}",
        range.start + 1,
        range.end,
        files.len()
    )
    .ok();
    ui!(
        AppBar::new("Files", battery),
        rows,
        Label::new(footer.as_str(), TextRole::Metadata).at(Point::new(CONTENT_LEFT, 714)),
        CommandBar::new(["Home", "Open", "Previous", "Next"]),
    )
    .draw(&mut display)
    .ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;

    use embedded_graphics::pixelcolor::GrayColor;

    use super::{FileItem, FileKind, render_files};
    use crate::{
        app::FilesState,
        image::{PackedImage, READER_DEPTH, Size},
        input::UsbState,
        power::BatteryStatus,
        ui::SELECTION_BACKGROUND,
    };

    #[test]
    fn every_file_row_uses_gray_selection_with_black_foreground_and_outline() {
        let files = [
            FileItem::new("alice.epub", 12_000, FileKind::Epub),
            FileItem::new("cover.jpg", 24_000, FileKind::Jpeg),
            FileItem::new("diagram.png", 36_000, FileKind::Png),
        ];
        let tops = [86, 162, 238];
        let foreground = [(46, 110), (36, 184), (36, 260)];
        let size = Size::new(480, 800).unwrap();
        let battery = BatteryStatus::from_percent(42, UsbState::Disconnected);

        for index in 0..files.len() {
            let mut bytes = std::vec![0xFF; READER_DEPTH.byte_len(size).unwrap()];
            let mut image = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
            render_files(
                FilesState::with_selected(files.len(), index).unwrap(),
                &files,
                battery,
                &mut image,
            )
            .unwrap();

            assert_eq!(
                image.luma(450, tops[index] + 31),
                SELECTION_BACKGROUND.luma(),
                "file row {index} lost its selection fill"
            );
            assert_eq!(
                image.luma(240, tops[index]),
                0,
                "file row {index} lost its outline"
            );
            assert_eq!(
                image.luma(foreground[index].0, foreground[index].1),
                0,
                "file row {index} lost its black icon"
            );
        }
    }
}
