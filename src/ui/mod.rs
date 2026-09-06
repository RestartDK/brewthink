mod app;
mod components;
mod frame;
mod layout;
mod theme;

pub use app::{AppFrame, AppRenderError, render_app};
pub use components::{ActionRow, AppBar, CommandBar, FileRow, Label, MenuRow, Selection, ValueRow};
pub use frame::{FixedText, FrameTarget};
pub(crate) use layout::{ui, ui_column};
pub use theme::{
    APP_BAR_RULE_Y, CONTENT_LEFT, CONTENT_TOP, CONTENT_WIDTH, FOOTER_RULE_Y, FOOTER_TEXT_Y,
    FRAME_HEIGHT, FRAME_WIDTH, TextRole, text_style,
};

#[cfg(test)]
mod tests {
    extern crate std;

    use embedded_graphics::Drawable;
    use std::vec;

    use super::{AppBar, FrameTarget};
    use crate::{
        image::{MonochromeBitmap, MonochromeImage, Size},
        input::UsbState,
        power::BatteryStatus,
    };

    fn render_battery(percent: u8, usb: UsbState) -> std::vec::Vec<u8> {
        let mut bytes = vec![0xFF; 480 * 800 / 8];
        let mut image = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();
        AppBar::new("HOME", BatteryStatus::from_percent(percent, usb))
            .draw(&mut FrameTarget::new(&mut image))
            .unwrap();
        bytes
    }

    #[test]
    fn external_power_hides_the_capacity_fill() {
        let bytes = render_battery(100, UsbState::Connected);
        let battery = MonochromeBitmap::new(Size::new(480, 800).unwrap(), &bytes).unwrap();

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
    fn external_power_symbol_stays_inside_the_battery() {
        let size = Size::new(480, 800).unwrap();
        for percent in [0, 50, 75, 100] {
            let connected_bytes = render_battery(percent, UsbState::Connected);
            let disconnected_bytes = render_battery(percent, UsbState::Disconnected);
            let connected = MonochromeBitmap::new(size, &connected_bytes).unwrap();
            let disconnected = MonochromeBitmap::new(size, &disconnected_bytes).unwrap();
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
