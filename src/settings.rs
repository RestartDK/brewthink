use embedded_graphics::{
    Drawable,
    geometry::{Point, Size as GraphicsSize},
    mono_font::MonoTextStyle,
    pixelcolor::BinaryColor,
    prelude::Primitive,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};

use crate::{
    app::{SettingsItem, SettingsState},
    image::{MonochromeImage, Size},
    power::BatteryStatus,
    reader::{ReaderStyle, ReaderTheme},
    ui::{
        CONTENT_LEFT, CONTENT_WIDTH, FRAME_HEIGHT, FRAME_WIDTH, FrameTarget, brand_style,
        chrome_style, draw_app_bar, draw_footer_rule,
    },
};

const ROW_TOPS: [i32; 4] = [92, 164, 236, 334];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsRenderError {
    WrongFrameSize { actual: Size },
}

pub fn render_settings(
    state: SettingsState,
    battery: BatteryStatus,
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

    Text::with_baseline(
        "READER PREVIEW",
        Point::new(CONTENT_LEFT, 448),
        chrome_style(),
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();
    Rectangle::new(
        Point::new(CONTENT_LEFT, 472),
        GraphicsSize::new(CONTENT_WIDTH, 202),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
    .draw(&mut display)
    .ok();

    let theme = ReaderTheme::from_preferences(state.draft());
    let style = MonoTextStyle::new(theme.font(ReaderStyle::Body), BinaryColor::On);
    let line_height = theme.line_height(ReaderStyle::Body) as i32;
    for (index, line) in [
        "A reader should disappear",
        "behind the words. Adjust",
        "the text until it feels right.",
    ]
    .into_iter()
    .enumerate()
    {
        Text::with_baseline(
            line,
            Point::new(36, 506 + index as i32 * line_height),
            style,
            Baseline::Top,
        )
        .draw(&mut display)
        .ok();
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

fn draw_row(display: &mut FrameTarget<'_, '_>, state: SettingsState, item: SettingsItem, top: i32) {
    let selected = state.selected() == item;
    let height = if item == SettingsItem::Apply { 74 } else { 58 };
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
        Point::new(34, top + if item == SettingsItem::Apply { 24 } else { 20 }),
        label_style,
        Baseline::Top,
    )
    .draw(display)
    .ok();

    let value = match item {
        SettingsItem::Font => state.draft().font().label(),
        SettingsItem::Size => state.draft().size().label(),
        SettingsItem::Spacing => state.draft().spacing().label(),
        SettingsItem::Apply => return,
    };
    Text::with_baseline(
        value,
        Point::new(350, top + 20),
        chrome_style(),
        Baseline::Top,
    )
    .draw(display)
    .ok();
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::render_settings;
    use crate::{
        app::{ReaderPreferences, SettingsState},
        image::{MonochromeImage, Size},
        input::UsbState,
        power::BatteryStatus,
    };

    #[test]
    fn keeps_application_chrome_and_reader_preview_separate() {
        let mut bytes = std::vec![0xFF; 480 * 800 / 8];
        let mut image = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();
        render_settings(
            SettingsState::new(ReaderPreferences::default()),
            BatteryStatus::from_percent(82, UsbState::Disconnected),
            &mut image,
        )
        .unwrap();
        assert!(image.pixel_is_black(18, 58));
        assert!(image.pixel_is_black(18, 92));
    }
}
