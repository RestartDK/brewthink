use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10, ascii::FONT_9X18_BOLD},
    pixelcolor::Gray8,
};

pub const FRAME_WIDTH: usize = 480;
pub const FRAME_HEIGHT: usize = 800;
pub const CONTENT_LEFT: i32 = 18;
pub const CONTENT_WIDTH: u32 = 444;
pub const APP_BAR_RULE_Y: i32 = 58;
pub const CONTENT_TOP: usize = 72;
pub const FOOTER_RULE_Y: i32 = 748;
pub const FOOTER_TEXT_Y: i32 = 768;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextRole {
    Brand,
    Section,
    Heading,
    Body,
    ControlLabel,
    Metadata,
    CommandHint,
    Error,
}

pub const fn text_style(role: TextRole) -> MonoTextStyle<'static, Gray8> {
    match role {
        TextRole::Brand | TextRole::Heading | TextRole::ControlLabel | TextRole::Error => {
            MonoTextStyle::new(&FONT_9X18_BOLD, Gray8::new(0))
        }
        TextRole::Section | TextRole::Body | TextRole::Metadata | TextRole::CommandHint => {
            MonoTextStyle::new(&FONT_6X10, Gray8::new(0))
        }
    }
}
