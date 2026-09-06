use embedded_graphics::{
    Drawable, Pixel,
    geometry::{Point, Size as GraphicsSize},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Primitive},
    primitives::{PrimitiveStyle, Rectangle},
};
use embedded_layout::View;

use crate::{
    app::{SettingsItem, SettingsState, SleepScreenMode},
    image::{MonochromeBitmap, MonochromeImage, Size},
    power::BatteryStatus,
    reader::{ReaderStyle, ReaderTheme},
    sleep::CustomSleepImageStatus,
    ui::{
        ActionRow, AppBar, CONTENT_LEFT, CONTENT_WIDTH, CommandBar, FRAME_HEIGHT, FRAME_WIDTH,
        FrameTarget, Label, Selection, TextRole, ValueRow, ui, ui_column,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsRenderError {
    WrongFrameSize { actual: Size },
}

pub fn render_settings(
    state: SettingsState,
    battery: BatteryStatus,
    custom_image_status: CustomSleepImageStatus,
    custom_image_name: Option<&str>,
    custom_image_preview: Option<MonochromeBitmap<'_>>,
    target: &mut MonochromeImage<'_>,
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
    let rows = ui_column!(
        20;
        ui_column!(
            10;
            value_row(SettingsItem::Font, preferences.reader().font().label(), selected),
            value_row(SettingsItem::Size, preferences.reader().size().label(), selected),
            value_row(SettingsItem::Spacing, preferences.reader().spacing().label(), selected),
            value_row(SettingsItem::SleepScreen, preferences.sleep_screen().label(), selected),
        ),
        ActionRow::new(
            SettingsItem::Apply.label(),
            Selection::from_selected(selected == SettingsItem::Apply),
        ),
    )
    .translate(Point::new(CONTENT_LEFT, 92));
    let screen = ui!(
        AppBar::new("SETTINGS", battery),
        rows,
        SettingsPreview {
            state,
            custom_image_status,
            custom_image_name,
            custom_image_preview,
            top_left: Point::new(CONTENT_LEFT, 416),
        },
        CommandBar::new("UP/DOWN  ROW     LEFT/RIGHT  CHANGE     BACK  CANCEL").at(
            CONTENT_LEFT,
            CONTENT_WIDTH,
            710,
            730,
        ),
    );
    screen.draw(&mut FrameTarget::new(target)).ok();
    Ok(())
}

fn value_row(item: SettingsItem, value: &'static str, selected: SettingsItem) -> ValueRow<'static> {
    ValueRow::new(
        item.label(),
        value,
        Selection::from_selected(item == selected),
    )
}

#[derive(Clone, Copy)]
struct SettingsPreview<'a> {
    state: SettingsState,
    custom_image_status: CustomSleepImageStatus,
    custom_image_name: Option<&'a str>,
    custom_image_preview: Option<MonochromeBitmap<'a>>,
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
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        let sleep_preview = self.state.selected() == SettingsItem::SleepScreen;
        let mode = self.state.draft().sleep_screen();
        let heading = if sleep_preview {
            match mode {
                SleepScreenMode::Automatic => "SLEEP PREVIEW  COVER IN READER, CUSTOM ELSEWHERE",
                SleepScreenMode::Custom => "SLEEP PREVIEW  CUSTOM IMAGE",
                SleepScreenMode::BookCover => "SLEEP PREVIEW  CURRENT BOOK COVER",
            }
        } else {
            "READER PREVIEW"
        };
        Label::new(heading, TextRole::Body)
            .at(self.top_left)
            .draw(target)?;
        Rectangle::new(
            self.top_left + Point::new(0, 24),
            GraphicsSize::new(CONTENT_WIDTH, 238),
        )
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(target)?;

        if sleep_preview && mode != SleepScreenMode::BookCover {
            if let Some(preview) = self.custom_image_preview {
                target.draw_iter((0..192).flat_map(|y| {
                    (0..128).map(move |x| {
                        Pixel(
                            self.top_left + Point::new(18 + x as i32, 40 + y as i32),
                            if preview.pixel_is_black(
                                x * preview.size().width() / 128,
                                y * preview.size().height() / 192,
                            ) {
                                BinaryColor::On
                            } else {
                                BinaryColor::Off
                            },
                        )
                    })
                }))?;
            } else {
                Label::new(self.custom_image_status.label(), TextRole::Heading)
                    .at(self.top_left + Point::new(30, 120))
                    .draw(target)?;
            }
            let message = match self.custom_image_status {
                CustomSleepImageStatus::Ready => self.custom_image_name.unwrap_or("IMAGE READY"),
                CustomSleepImageStatus::Missing => "Upload a JPG or PNG over USB",
                CustomSleepImageStatus::Invalid => "Replace the selected image",
            };
            Label::new(message, TextRole::Body)
                .at(self.top_left + Point::new(172, 100))
                .draw(target)?;
            return Label::new(self.custom_image_status.label(), TextRole::Heading)
                .at(self.top_left + Point::new(172, 132))
                .draw(target);
        }

        let lines = if sleep_preview {
            [
                "The selected book cover is used",
                "while browsing or reading.",
                "Built-in is the safe fallback.",
            ]
        } else {
            [
                "A reader should disappear",
                "behind the words. Adjust",
                "the text until it feels right.",
            ]
        };
        let theme = ReaderTheme::from_preferences(self.state.draft().reader());
        let line_height = theme.line_height(ReaderStyle::Body) as i32;
        for (index, line) in lines.into_iter().enumerate() {
            theme.draw_text(
                line,
                ReaderStyle::Body,
                self.top_left + Point::new(30, 60 + index as i32 * line_height),
                target,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::render_settings;
    use crate::{
        app::{AppPreferences, SettingsItem, SettingsState},
        image::{MonochromeImage, Size},
        input::UsbState,
        power::BatteryStatus,
        sleep::CustomSleepImageStatus,
    };

    #[test]
    fn renders_every_settings_row_and_missing_image_state() {
        let mut bytes = std::vec![0xFF; 480 * 800 / 8];
        let mut image = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();
        render_settings(
            SettingsState::with_state(SettingsItem::SleepScreen, AppPreferences::default()),
            BatteryStatus::from_percent(82, UsbState::Disconnected),
            CustomSleepImageStatus::Missing,
            None,
            None,
            &mut image,
        )
        .unwrap();
        assert!(image.pixel_is_black(18, 58));
        assert!(image.pixel_is_black(18, 92));
        assert!(image.pixel_is_black(18, 326));
    }
}
