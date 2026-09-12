mod app;
mod components;
mod drawer;
#[cfg(test)]
mod drawer_tests;
mod frame;
mod icons;
mod layout;
mod reader_drawer;
mod theme;

pub use app::{AppFrame, AppRenderError, render_app};
pub use components::{AppBar, CommandBar, FileRow, Label, MenuRow, Selection, SettingsRow};
pub use drawer::DrawerSurface;
pub use frame::{FixedText, FrameTarget};
pub use icons::Icon;
pub(crate) use layout::{ui, ui_column};
pub use reader_drawer::draw_reader_drawer;
pub(crate) use theme::text_font;
pub use theme::{
    APP_BAR_RULE_Y, CHROME_INK, CHROME_PAPER, CONTENT_LEFT, CONTENT_TOP, CONTENT_WIDTH,
    FOOTER_RULE_Y, FOOTER_TEXT_Y, FRAME_HEIGHT, FRAME_WIDTH, FRONT_BUTTON_CENTERS, PANEL_CORNERS,
    ROW_CORNERS, SELECTION_BACKGROUND, SELECTION_FOREGROUND, SELECTION_OUTLINE, TextRole,
    text_width,
};

#[cfg(test)]
mod tests {
    extern crate std;

    use embedded_graphics::{Drawable, pixelcolor::GrayColor};
    use std::vec;

    use super::{AppBar, FrameTarget};
    use crate::{
        image::{PackedBitmap, PackedImage, READER_DEPTH, Size},
        input::UsbState,
        power::BatteryStatus,
    };

    fn render_battery(percent: u8, usb: UsbState) -> std::vec::Vec<u8> {
        let size = Size::new(480, 800).unwrap();
        let mut bytes = vec![0xFF; READER_DEPTH.byte_len(size).unwrap()];
        let mut image = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
        AppBar::new("HOME", BatteryStatus::from_percent(percent, usb))
            .draw(&mut FrameTarget::new(&mut image))
            .unwrap();
        bytes
    }

    #[test]
    fn command_bar_places_navigation_left_and_actions_right() {
        use super::{CHROME_INK, CommandBar, Icon, Label, TextRole, text_width};
        use embedded_graphics::geometry::{Point, Size as GraphicsSize};

        let size = Size::new(480, 800).unwrap();
        let mut actual = vec![0xff; READER_DEPTH.byte_len(size).unwrap()];
        let mut expected = actual.clone();
        let mut image = PackedImage::new(size, READER_DEPTH, &mut actual).unwrap();
        CommandBar::new(["Cancel", "Go to", "Decrease", "Increase"])
            .draw(&mut FrameTarget::new(&mut image))
            .unwrap();
        let mut reference = PackedImage::new(size, READER_DEPTH, &mut expected).unwrap();
        let mut target = FrameTarget::new(&mut reference);
        for (icon, label, center) in [
            (Icon::Left, "Decrease", 100),
            (Icon::Right, "Increase", 192),
            (Icon::Back, "Cancel", 300),
            (Icon::Confirm, "Go to", 392),
        ] {
            icon.draw(&mut target, Point::new(center - 12, 738), CHROME_INK)
                .unwrap();
            Label::new(label, TextRole::CommandHint)
                .at(Point::new(
                    center - text_width(TextRole::CommandHint, label) as i32 / 2,
                    770,
                ))
                .clipped_to(GraphicsSize::new(92, 22))
                .draw(&mut target)
                .unwrap();
        }
        for y in 731..800 {
            for x in 0..480 {
                assert_eq!(
                    image.luma(x, y),
                    reference.luma(x, y),
                    "footer pixel {x},{y}"
                );
            }
        }
    }

    #[test]
    fn selection_palette_uses_light_gray_with_black_foreground_and_outline() {
        assert_eq!(super::SELECTION_BACKGROUND.luma(), 170);
        assert_eq!(super::SELECTION_FOREGROUND.luma(), 0);
        assert_eq!(super::SELECTION_OUTLINE.luma(), 0);
    }

    #[test]
    fn external_power_hides_the_capacity_fill() {
        let bytes = render_battery(100, UsbState::Connected);
        let battery =
            PackedBitmap::new(Size::new(480, 800).unwrap(), READER_DEPTH, &bytes).unwrap();

        for y in 22..31 {
            for x in (376..384).chain(393..399) {
                assert!(
                    !battery.pixel_is_black(x, y),
                    "battery fill remained at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn clipped_labels_ellipsize_at_glyph_boundaries_in_both_colors() {
        use super::{CHROME_INK, CHROME_PAPER, Label, TextRole, text_width};
        use embedded_graphics::geometry::{Point, Size as GraphicsSize};
        for (text, prefix) in [("WWWWWWWW", "WWW…"), ("éééééééé", "ééé…")] {
            for (color, background) in [(CHROME_INK, 0xff), (CHROME_PAPER, 0x00)] {
                let width = text_width(TextRole::ControlLabel, prefix) as u32;
                let render = |text: &str| {
                    let size = Size::new(480, 800).unwrap();
                    let mut bytes = vec![background; READER_DEPTH.byte_len(size).unwrap()];
                    let mut image = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
                    Label::new(text, TextRole::ControlLabel)
                        .at(Point::new(18, 90))
                        .color(color)
                        .clipped_to(GraphicsSize::new(width, 34))
                        .draw(&mut FrameTarget::new(&mut image))
                        .unwrap();
                    bytes
                };
                assert_eq!(render(text), render(prefix));
            }
        }
    }

    #[test]
    fn external_power_symbol_stays_inside_the_battery() {
        let size = Size::new(480, 800).unwrap();
        for percent in [0, 50, 75, 100] {
            let connected_bytes = render_battery(percent, UsbState::Connected);
            let disconnected_bytes = render_battery(percent, UsbState::Disconnected);
            let connected = PackedBitmap::new(size, READER_DEPTH, &connected_bytes).unwrap();
            let disconnected = PackedBitmap::new(size, READER_DEPTH, &disconnected_bytes).unwrap();
            let mut inside = 0;
            let mut outside = 0;

            for y in 0..800 {
                for x in 0..480 {
                    if connected.pixel_is_black(x, y) == disconnected.pixel_is_black(x, y) {
                        continue;
                    }
                    if (375..400).contains(&x) && (21..32).contains(&y) {
                        inside += 1;
                    } else {
                        outside += 1;
                    }
                }
            }

            assert!(inside > 0, "power symbol disappeared at {percent}%");
            assert_eq!(outside, 0, "power symbol escaped the battery at {percent}%");
        }
    }
}
