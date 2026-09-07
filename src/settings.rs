use embedded_graphics::{
    Drawable, Pixel,
    geometry::{Point, Size as GraphicsSize},
    pixelcolor::Gray8,
    prelude::{DrawTarget, Primitive},
    primitives::{PrimitiveStyle, Rectangle, RoundedRectangle},
};
use embedded_layout::{
    View,
    layout::linear::{FixedMargin, LinearLayout},
    view_group::Views,
};

use crate::{
    app::{SettingsItem, SettingsState, SleepScreenMode},
    image::{PackedBitmap, PackedImage, Size},
    power::BatteryStatus,
    reader::{ReaderStyle, ReaderTheme},
    ui::{
        AppBar, CONTENT_LEFT, CONTENT_WIDTH, CommandBar, FRAME_HEIGHT, FRAME_WIDTH, FrameTarget,
        Label, Selection, SettingsRow, TextRole, ui,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustomImagePreview<'a> {
    Missing,
    Invalid,
    Ready {
        name: &'a str,
        bitmap: PackedBitmap<'a>,
    },
}

impl CustomImagePreview<'_> {
    const fn label(self) -> &'static str {
        match self {
            Self::Missing => "No image",
            Self::Invalid => "Invalid image",
            Self::Ready { .. } => "Image ready",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsRenderError {
    WrongFrameSize { actual: Size },
}

pub fn render_settings(
    state: SettingsState,
    battery: BatteryStatus,
    custom_image: CustomImagePreview<'_>,
    target: &mut PackedImage<'_>,
) -> Result<(), SettingsRenderError> {
    let expected =
        Size::new(FRAME_WIDTH, FRAME_HEIGHT).expect("settings frame dimensions are non-zero");
    if target.size() != expected {
        return Err(SettingsRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }

    target.clear_white();
    let selected = state.selected();
    let preferences = state.draft();
    let mut row_views = SettingsItem::ALL.map(|item| {
        SettingsRow::new(
            item,
            item.value(preferences),
            Selection::from_selected(item == selected),
        )
    });
    let rows = LinearLayout::vertical(Views::new(&mut row_views))
        .with_spacing(FixedMargin(10))
        .arrange()
        .translate(Point::new(CONTENT_LEFT, 92));
    let screen = ui!(
        AppBar::new("Settings", battery),
        rows,
        SettingsPreview {
            state,
            custom_image,
            top_left: Point::new(CONTENT_LEFT, 416),
        },
        Label::new("Side buttons: choose a row", TextRole::Metadata)
            .at(Point::new(CONTENT_LEFT, 714)),
        CommandBar::new([
            "Cancel",
            if selected == SettingsItem::Apply {
                "Save"
            } else {
                "Change"
            },
            if selected == SettingsItem::Apply {
                ""
            } else {
                "Decrease"
            },
            if selected == SettingsItem::Apply {
                ""
            } else {
                "Increase"
            }
        ]),
    );
    screen.draw(&mut FrameTarget::new(target)).ok();
    Ok(())
}

#[derive(Clone, Copy)]
struct SettingsPreview<'a> {
    state: SettingsState,
    custom_image: CustomImagePreview<'a>,
    top_left: Point,
}

impl View for SettingsPreview<'_> {
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

    fn bounds(&self) -> Rectangle {
        Rectangle::new(self.top_left, GraphicsSize::new(CONTENT_WIDTH, 262))
    }
}

impl Drawable for SettingsPreview<'_> {
    type Color = Gray8;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        let sleep_preview = self.state.selected() == SettingsItem::SleepScreen;
        let mode = self.state.draft().sleep_screen();
        let heading = if sleep_preview {
            match mode {
                SleepScreenMode::Automatic => "Sleep preview: cover in reader, image elsewhere",
                SleepScreenMode::Custom => "Sleep preview: custom image",
                SleepScreenMode::BookCover => "Sleep preview: book cover",
            }
        } else {
            "Reader preview"
        };
        Label::new(heading, TextRole::Body)
            .at(self.top_left)
            .draw(target)?;
        RoundedRectangle::with_equal_corners(
            Rectangle::new(
                self.top_left + Point::new(0, 24),
                GraphicsSize::new(CONTENT_WIDTH, 238),
            ),
            crate::ui::PANEL_CORNERS,
        )
        .into_styled(PrimitiveStyle::with_stroke(Gray8::new(0), 1))
        .draw(target)?;

        if sleep_preview && mode != SleepScreenMode::BookCover {
            if let CustomImagePreview::Ready {
                bitmap: preview, ..
            } = self.custom_image
            {
                target.draw_iter((0..192).flat_map(|y| {
                    (0..128).map(move |x| {
                        Pixel(
                            self.top_left + Point::new(18 + x as i32, 40 + y as i32),
                            Gray8::new(preview.luma(
                                x * preview.size().width() / 128,
                                y * preview.size().height() / 192,
                            )),
                        )
                    })
                }))?;
            } else {
                crate::ui::Icon::Image.draw(
                    target,
                    self.top_left + Point::new(70, 124),
                    crate::ui::CHROME_INK,
                )?;
            }
            let message = match self.custom_image {
                CustomImagePreview::Ready { name, .. } => name,
                CustomImagePreview::Missing => "Upload a JPG or PNG over USB",
                CustomImagePreview::Invalid => "Replace the selected image",
            };
            Label::new(message, TextRole::Body)
                .at(self.top_left + Point::new(172, 136))
                .clipped_to(GraphicsSize::new(254, 28))
                .draw(target)?;
            return Label::new(self.custom_image.label(), TextRole::Heading)
                .at(self.top_left + Point::new(172, 96))
                .draw(target);
        }

        if sleep_preview {
            for (index, line) in [
                "Your book cover appears here.",
                "Without a readable cover,",
                "the built-in sleep screen is used.",
            ]
            .into_iter()
            .enumerate()
            {
                Label::new(line, TextRole::Body)
                    .at(self.top_left + Point::new(30, 70 + index as i32 * 30))
                    .draw(target)?;
            }
            return Ok(());
        }
        let theme = ReaderTheme::from_preferences(self.state.draft().reader());
        let line_height = theme.line_height(ReaderStyle::Body) as i32;
        let mut remaining =
            "A reader should disappear behind the words. Adjust the text until it feels right.";
        let mut y = 60;
        while !remaining.is_empty() && y + line_height <= 254 {
            let end = remaining
                .char_indices()
                .find_map(|(index, character)| {
                    (theme.text_width(
                        ReaderStyle::Body,
                        &remaining[..index + character.len_utf8()],
                    ) > CONTENT_WIDTH as usize - 60)
                        .then_some(index)
                })
                .unwrap_or(remaining.len());
            if end == 0 {
                break;
            }
            let end = if end < remaining.len() {
                remaining[..end].rfind(char::is_whitespace).unwrap_or(end)
            } else {
                end
            };
            theme.draw_text(
                &remaining[..end],
                ReaderStyle::Body,
                self.top_left + Point::new(30, y),
                target,
            )?;
            remaining = remaining[end..].trim_start();
            y += line_height;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use embedded_graphics::pixelcolor::GrayColor;

    use super::{CustomImagePreview, render_settings};
    use crate::{
        app::{AppPreferences, SettingsItem, SettingsState},
        image::{PackedImage, READER_DEPTH, Size},
        input::UsbState,
        power::BatteryStatus,
        ui::SELECTION_BACKGROUND,
    };

    #[test]
    fn every_reader_preference_keeps_the_preview_inside_its_panel() {
        use crate::app::ReaderPreferences;
        for font in 0..3 {
            for size in 0..3 {
                for spacing in 0..3 {
                    let reader =
                        ReaderPreferences::from_packed(font | (size << 8) | (spacing << 16))
                            .unwrap();
                    let preferences =
                        AppPreferences::new(reader, crate::app::SleepScreenMode::Automatic);
                    let size = Size::new(480, 800).unwrap();
                    let mut bytes = std::vec![0xff; READER_DEPTH.byte_len(size).unwrap()];
                    let mut frame = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
                    render_settings(
                        SettingsState::new(preferences),
                        BatteryStatus::from_percent(82, UsbState::Disconnected),
                        CustomImagePreview::Missing,
                        &mut frame,
                    )
                    .unwrap();
                    for y in 440..710 {
                        for x in 464..480 {
                            assert!(!frame.pixel_is_black(x, y), "preview overflow at {x},{y}");
                        }
                    }
                    for y in 680..710 {
                        for x in 18..462 {
                            assert!(!frame.pixel_is_black(x, y), "preview overflow at {x},{y}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_settings_row_uses_gray_selection_with_black_foreground_and_outline() {
        let tops = [92, 148, 204, 260, 326];
        let heights = [46, 46, 46, 46, 64];
        let foreground = [(41, 110), (40, 179), (49, 219), (51, 273), (42, 361)];
        let size = Size::new(480, 800).unwrap();
        let battery = BatteryStatus::from_percent(82, UsbState::Disconnected);

        for (index, item) in SettingsItem::ALL.into_iter().enumerate() {
            let mut bytes = std::vec![0xFF; READER_DEPTH.byte_len(size).unwrap()];
            let mut image = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
            render_settings(
                SettingsState::with_state(item, AppPreferences::default()),
                battery,
                CustomImagePreview::Missing,
                &mut image,
            )
            .unwrap();

            assert_eq!(
                image.luma(450, tops[index] + heights[index] / 2),
                SELECTION_BACKGROUND.luma(),
                "settings row {index} lost its selection fill"
            );
            assert_eq!(
                image.luma(240, tops[index]),
                0,
                "settings row {index} lost its outline"
            );
            assert_eq!(
                image.luma(foreground[index].0, foreground[index].1),
                0,
                "settings row {index} lost its black icon"
            );
        }
    }
}
