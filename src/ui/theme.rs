use embedded_graphics::pixelcolor::Gray8;

use crate::fonts::{
    BitmapFont,
    noto_sans::{
        NOTO_SANS_14_REGULAR, NOTO_SANS_18_REGULAR, NOTO_SANS_22_REGULAR, NOTO_SANS_24_SEMIBOLD,
    },
};

pub const FRAME_WIDTH: usize = 480;
pub const FRAME_HEIGHT: usize = 800;
pub const CONTENT_LEFT: i32 = 18;
pub const CONTENT_WIDTH: u32 = 444;
pub const APP_BAR_RULE_Y: i32 = 58;
pub const CONTENT_TOP: usize = 72;
pub const FOOTER_RULE_Y: i32 = 730;
pub const FOOTER_TEXT_Y: i32 = 770;
pub const ROW_CORNERS: embedded_graphics::geometry::Size =
    embedded_graphics::geometry::Size::new(12, 12);
pub const PANEL_CORNERS: embedded_graphics::geometry::Size =
    embedded_graphics::geometry::Size::new(28, 28);
pub const FRONT_BUTTON_CENTERS: [i32; 4] = [100, 192, 300, 392];
pub const CHROME_INK: Gray8 = Gray8::new(0);
pub const CHROME_PAPER: Gray8 = Gray8::new(255);
pub const SELECTION_BACKGROUND: Gray8 = Gray8::new(170);
pub const SELECTION_FOREGROUND: Gray8 = CHROME_INK;
pub const SELECTION_OUTLINE: Gray8 = CHROME_INK;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextRole {
    Section,
    Heading,
    Body,
    ControlLabel,
    Metadata,
    CommandHint,
    Error,
}

pub(crate) const fn text_font(role: TextRole) -> &'static BitmapFont {
    match role {
        TextRole::Section | TextRole::Heading | TextRole::Error => &NOTO_SANS_24_SEMIBOLD,
        TextRole::ControlLabel => &NOTO_SANS_22_REGULAR,
        TextRole::Body => &NOTO_SANS_18_REGULAR,
        TextRole::Metadata | TextRole::CommandHint => &NOTO_SANS_14_REGULAR,
    }
}

pub fn text_width(role: TextRole, text: &str) -> usize {
    text_font(role).text_width(text)
}
