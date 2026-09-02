use core::fmt::Write;

use embedded_graphics::{
    Drawable,
    draw_target::DrawTargetExt,
    geometry::{Point, Size as GraphicsSize},
    pixelcolor::BinaryColor,
    prelude::Primitive,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};

use crate::{
    app::FilesState,
    image::{MonochromeImage, Size},
    power::BatteryStatus,
    ui::{
        CONTENT_LEFT, CONTENT_WIDTH, FRAME_HEIGHT, FRAME_WIDTH, FixedText, FrameTarget,
        brand_style, chrome_style, draw_app_bar, draw_footer_rule,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileItem<'a> {
    name: &'a str,
    size: u32,
}

impl<'a> FileItem<'a> {
    pub const fn new(name: &'a str, size: u32) -> Self {
        Self { name, size }
    }

    pub const fn name(self) -> &'a str {
        self.name
    }

    pub const fn size(self) -> u32 {
        self.size
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
    target: &mut MonochromeImage<'_>,
) -> Result<(), FilesRenderError> {
    let expected =
        Size::new(FRAME_WIDTH, FRAME_HEIGHT).expect("files frame dimensions are non-zero");
    if target.size() != expected {
        return Err(FilesRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }
    if state.book_count() != files.len() {
        return Err(FilesRenderError::CatalogLengthMismatch {
            state: state.book_count(),
            files: files.len(),
        });
    }

    target.clear_white();
    draw_app_bar(target, "FILES  /BOOKS", battery);
    if files.is_empty() {
        draw_empty_state(target);
        return Ok(());
    }

    let mut display = FrameTarget::new(target);
    let range = state.visible_range();
    for (row, index) in range.clone().enumerate() {
        let top = 86 + row as i32 * 76;
        let selected = state
            .selected()
            .is_some_and(|selected| selected.index() == index);
        Rectangle::new(
            Point::new(CONTENT_LEFT, top),
            GraphicsSize::new(CONTENT_WIDTH, 62),
        )
        .into_styled(PrimitiveStyle::with_stroke(
            BinaryColor::On,
            if selected { 3 } else { 1 },
        ))
        .draw(&mut display)
        .ok();

        let name_clip = Rectangle::new(Point::new(32, top + 12), GraphicsSize::new(330, 18));
        Text::with_baseline(
            files[index].name,
            Point::new(32, top + 12),
            brand_style(),
            Baseline::Top,
        )
        .draw(&mut display.clipped(&name_clip))
        .ok();

        let mut size = FixedText::<24>::new();
        write!(size, "{} KiB", files[index].size.div_ceil(1024)).ok();
        Text::with_baseline(
            size.as_str(),
            Point::new(382, top + 20),
            chrome_style(),
            Baseline::Top,
        )
        .draw(&mut display)
        .ok();
        Text::with_baseline(
            "EPUB",
            Point::new(32, top + 39),
            chrome_style(),
            Baseline::Top,
        )
        .draw(&mut display)
        .ok();
    }

    draw_footer_rule(target, 748);
    let mut footer = FixedText::<64>::new();
    write!(
        footer,
        "{}-{} / {}     CONFIRM  OPEN     BACK  HOME",
        range.start + 1,
        range.end,
        files.len()
    )
    .ok();
    Text::with_baseline(
        footer.as_str(),
        Point::new(CONTENT_LEFT, 768),
        chrome_style(),
        Baseline::Top,
    )
    .draw(&mut FrameTarget::new(target))
    .ok();
    Ok(())
}

fn draw_empty_state(target: &mut MonochromeImage<'_>) {
    let mut display = FrameTarget::new(target);
    Text::with_baseline(
        "NO EPUB FILES",
        Point::new(166, 326),
        brand_style(),
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();
    Text::with_baseline(
        "Add DRM-free EPUB files to /Books",
        Point::new(135, 368),
        chrome_style(),
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();
    draw_footer_rule(target, 748);
    Text::with_baseline(
        "BACK  HOME",
        Point::new(CONTENT_LEFT, 768),
        chrome_style(),
        Baseline::Top,
    )
    .draw(&mut FrameTarget::new(target))
    .ok();
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{FileItem, render_files};
    use crate::{
        app::FilesState,
        image::{MonochromeImage, Size},
        input::UsbState,
        power::BatteryStatus,
    };

    #[test]
    fn renders_selected_files_with_sizes() {
        let mut bytes = std::vec![0xFF; 480 * 800 / 8];
        let mut image = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();
        render_files(
            FilesState::new(2),
            &[
                FileItem::new("alice.epub", 12_000),
                FileItem::new("walden.epub", 24_000),
            ],
            BatteryStatus::from_percent(42, UsbState::Disconnected),
            &mut image,
        )
        .unwrap();
        assert!(image.pixel_is_black(18, 86));
        assert!(image.pixel_is_black(18, 148));
    }
}
