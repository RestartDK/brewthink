use embedded_graphics::{
    Drawable,
    geometry::{Point, Size as GraphicsSize},
    pixelcolor::BinaryColor,
    prelude::Primitive,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};

use crate::{
    app::{SettingsItem, SettingsState, SleepScreenMode},
    image::{MonochromeBitmap, MonochromeImage, Size},
    power::BatteryStatus,
    reader::{ReaderStyle, ReaderTheme},
    sleep::CustomSleepImageStatus,
    ui::{
        CONTENT_LEFT, CONTENT_WIDTH, FRAME_HEIGHT, FRAME_WIDTH, FrameTarget, brand_style,
        chrome_style, draw_app_bar, draw_footer_rule,
    },
};

const ROW_TOPS: [i32; 5] = [92, 148, 204, 260, 326];

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
    draw_app_bar(target, "SETTINGS", battery);
    let mut display = FrameTarget::new(target);
    for (index, item) in SettingsItem::ALL.into_iter().enumerate() {
        draw_row(&mut display, state, item, ROW_TOPS[index]);
    }

    let sleep_preview = state.selected() == SettingsItem::SleepScreen;
    let preview_heading = if sleep_preview {
        match state.draft().sleep_screen() {
            SleepScreenMode::Automatic => "SLEEP PREVIEW  COVER IN READER, CUSTOM ELSEWHERE",
            SleepScreenMode::Custom => "SLEEP PREVIEW  CUSTOM IMAGE",
            SleepScreenMode::BookCover => "SLEEP PREVIEW  CURRENT BOOK COVER",
        }
    } else {
        "READER PREVIEW"
    };
    Text::with_baseline(
        preview_heading,
        Point::new(CONTENT_LEFT, 416),
        chrome_style(),
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();
    Rectangle::new(
        Point::new(CONTENT_LEFT, 440),
        GraphicsSize::new(CONTENT_WIDTH, 238),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
    .draw(&mut display)
    .ok();

    if sleep_preview {
        match state.draft().sleep_screen() {
            SleepScreenMode::Custom | SleepScreenMode::Automatic => {
                if let Some(preview) = custom_image_preview {
                    draw_preview_bitmap(preview, target, 36, 456, 128, 192);
                } else {
                    Text::with_baseline(
                        custom_image_status.label(),
                        Point::new(48, 536),
                        brand_style(),
                        Baseline::Top,
                    )
                    .draw(&mut FrameTarget::new(target))
                    .ok();
                }
                let message = match custom_image_status {
                    CustomSleepImageStatus::Ready => custom_image_name.unwrap_or("IMAGE READY"),
                    CustomSleepImageStatus::Missing => "Upload a JPG or PNG over USB",
                    CustomSleepImageStatus::Invalid => "Replace the selected image",
                };
                Text::with_baseline(message, Point::new(190, 516), chrome_style(), Baseline::Top)
                    .draw(&mut FrameTarget::new(target))
                    .ok();
                Text::with_baseline(
                    custom_image_status.label(),
                    Point::new(190, 548),
                    brand_style(),
                    Baseline::Top,
                )
                .draw(&mut FrameTarget::new(target))
                .ok();
            }
            SleepScreenMode::BookCover => draw_preview_text(
                state,
                [
                    "The selected book cover is used",
                    "while browsing or reading.",
                    "Built-in is the safe fallback.",
                ],
                target,
            ),
        }
    } else {
        draw_preview_text(
            state,
            [
                "A reader should disappear",
                "behind the words. Adjust",
                "the text until it feels right.",
            ],
            target,
        );
    }

    draw_footer_rule(target, 710);
    Text::with_baseline(
        "UP/DOWN  ROW     LEFT/RIGHT  CHANGE     BACK  CANCEL",
        Point::new(CONTENT_LEFT, 730),
        chrome_style(),
        Baseline::Top,
    )
    .draw(&mut FrameTarget::new(target))
    .ok();
    Ok(())
}

fn draw_preview_text(state: SettingsState, lines: [&str; 3], target: &mut MonochromeImage<'_>) {
    let theme = ReaderTheme::from_preferences(state.draft().reader());
    for (index, line) in lines.into_iter().enumerate() {
        theme
            .draw_text(
                line,
                ReaderStyle::Body,
                Point::new(
                    48,
                    476 + index as i32 * theme.line_height(ReaderStyle::Body) as i32,
                ),
                &mut FrameTarget::new(target),
            )
            .ok();
    }
}

fn draw_row(display: &mut FrameTarget<'_, '_>, state: SettingsState, item: SettingsItem, top: i32) {
    let selected = state.selected() == item;
    let height = if item == SettingsItem::Apply { 64 } else { 46 };
    Rectangle::new(
        Point::new(CONTENT_LEFT, top),
        GraphicsSize::new(CONTENT_WIDTH, height),
    )
    .into_styled(PrimitiveStyle::with_stroke(
        BinaryColor::On,
        if selected { 3 } else { 1 },
    ))
    .draw(display)
    .ok();

    let label_style = if item == SettingsItem::Apply {
        brand_style()
    } else {
        chrome_style()
    };
    Text::with_baseline(
        item.label(),
        Point::new(34, top + if item == SettingsItem::Apply { 20 } else { 14 }),
        label_style,
        Baseline::Top,
    )
    .draw(display)
    .ok();

    let value = match item {
        SettingsItem::Font => state.draft().reader().font().label(),
        SettingsItem::Size => state.draft().reader().size().label(),
        SettingsItem::Spacing => state.draft().reader().spacing().label(),
        SettingsItem::SleepScreen => state.draft().sleep_screen().label(),
        SettingsItem::Apply => return,
    };
    Text::with_baseline(
        value,
        Point::new(330, top + 14),
        chrome_style(),
        Baseline::Top,
    )
    .draw(display)
    .ok();
}

fn draw_preview_bitmap(
    source: MonochromeBitmap<'_>,
    target: &mut MonochromeImage<'_>,
    left: usize,
    top: usize,
    width: usize,
    height: usize,
) {
    for y in 0..height {
        let source_y = y * source.size().height() / height;
        for x in 0..width {
            let source_x = x * source.size().width() / width;
            target.set_pixel(left + x, top + y, source.pixel_is_black(source_x, source_y));
        }
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
