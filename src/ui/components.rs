use core::fmt::Write;

use embedded_graphics::{
    Drawable, Pixel,
    draw_target::DrawTargetExt,
    geometry::{Dimensions, Point, Size},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Primitive},
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};
use embedded_layout::View;

use crate::power::{BatteryLevel, BatteryStatus};

use super::{
    APP_BAR_RULE_Y, CONTENT_LEFT, CONTENT_WIDTH, FOOTER_RULE_Y, FOOTER_TEXT_Y, FRAME_WIDTH,
    FixedText, TextRole, text_style,
};

const POWER_SYMBOL_X: i32 = 386;
const POWER_SYMBOL_Y: i32 = 22;
const POWER_SYMBOL_ROWS: [u8; 9] = [
    0b00011, 0b00110, 0b01110, 0b11111, 0b00110, 0b01110, 0b01100, 0b11000, 0b10000,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Selection {
    Idle,
    Selected,
}

impl Selection {
    pub const fn from_selected(selected: bool) -> Self {
        if selected { Self::Selected } else { Self::Idle }
    }

    pub const fn stroke(self, idle: u32, selected: u32) -> u32 {
        match self {
            Self::Idle => idle,
            Self::Selected => selected,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Label<'a> {
    text: &'a str,
    role: TextRole,
    top_left: Point,
    clip: Option<Size>,
}

impl<'a> Label<'a> {
    pub const fn new(text: &'a str, role: TextRole) -> Self {
        Self {
            text,
            role,
            top_left: Point::zero(),
            clip: None,
        }
    }

    pub const fn at(mut self, top_left: Point) -> Self {
        self.top_left = top_left;
        self
    }

    pub const fn clipped_to(mut self, size: Size) -> Self {
        self.clip = Some(size);
        self
    }
}

impl View for Label<'_> {
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

    fn bounds(&self) -> Rectangle {
        self.clip.map_or_else(
            || {
                Text::with_baseline(
                    self.text,
                    self.top_left,
                    text_style(self.role),
                    Baseline::Top,
                )
                .bounding_box()
            },
            |size| Rectangle::new(self.top_left, size),
        )
    }
}

impl Drawable for Label<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        let text = Text::with_baseline(
            self.text,
            self.top_left,
            text_style(self.role),
            Baseline::Top,
        );
        match self.clip {
            Some(size) => text
                .draw(&mut target.clipped(&Rectangle::new(self.top_left, size)))
                .map(|_| ()),
            None => text.draw(target).map(|_| ()),
        }
    }
}

#[derive(Clone, Copy)]
pub struct AppBar<'a> {
    section: &'a str,
    battery: BatteryStatus,
    top_left: Point,
}

impl<'a> AppBar<'a> {
    pub const fn new(section: &'a str, battery: BatteryStatus) -> Self {
        Self {
            section,
            battery,
            top_left: Point::zero(),
        }
    }
}

impl View for AppBar<'_> {
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

    fn bounds(&self) -> Rectangle {
        Rectangle::new(
            self.top_left,
            Size::new(FRAME_WIDTH as u32, (APP_BAR_RULE_Y + 2) as u32),
        )
    }
}

impl Drawable for AppBar<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        Label::new("BREWTHINK", TextRole::Brand)
            .at(self.top_left + Point::new(CONTENT_LEFT, 14))
            .draw(target)?;
        Label::new(self.section, TextRole::Section)
            .at(self.top_left + Point::new(CONTENT_LEFT, 39))
            .clipped_to(Size::new(330, 12))
            .draw(target)?;
        Rectangle::new(
            self.top_left + Point::new(CONTENT_LEFT, APP_BAR_RULE_Y),
            Size::new(CONTENT_WIDTH, 2),
        )
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(target)?;
        draw_battery(target, self.top_left, self.battery)
    }
}

#[derive(Clone, Copy)]
pub struct CommandBar<'a> {
    text: &'a str,
    left: i32,
    width: u32,
    rule_y: i32,
    text_y: i32,
}

impl<'a> CommandBar<'a> {
    pub const fn new(text: &'a str) -> Self {
        Self {
            text,
            left: CONTENT_LEFT,
            width: CONTENT_WIDTH,
            rule_y: FOOTER_RULE_Y,
            text_y: FOOTER_TEXT_Y,
        }
    }

    pub const fn at(mut self, left: i32, width: u32, rule_y: i32, text_y: i32) -> Self {
        self.left = left;
        self.width = width;
        self.rule_y = rule_y;
        self.text_y = text_y;
        self
    }
}

impl View for CommandBar<'_> {
    fn translate_impl(&mut self, by: Point) {
        self.left += by.x;
        self.rule_y += by.y;
        self.text_y += by.y;
    }

    fn bounds(&self) -> Rectangle {
        Rectangle::new(
            Point::new(self.left, self.rule_y),
            Size::new(self.width, (self.text_y - self.rule_y + 10) as u32),
        )
    }
}

impl Drawable for CommandBar<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        Rectangle::new(Point::new(self.left, self.rule_y), Size::new(self.width, 1))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(target)?;
        Label::new(self.text, TextRole::CommandHint)
            .at(Point::new(self.left, self.text_y))
            .draw(target)
    }
}

#[derive(Clone, Copy)]
pub struct MenuRow<'a> {
    title: &'a str,
    detail: &'a str,
    selection: Selection,
    top_left: Point,
}

impl<'a> MenuRow<'a> {
    pub const fn new(title: &'a str, detail: &'a str, selection: Selection) -> Self {
        Self {
            title,
            detail,
            selection,
            top_left: Point::zero(),
        }
    }
}

impl View for MenuRow<'_> {
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

    fn bounds(&self) -> Rectangle {
        Rectangle::new(self.top_left, Size::new(CONTENT_WIDTH, 92))
    }
}

impl Drawable for MenuRow<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        self.bounds()
            .into_styled(PrimitiveStyle::with_stroke(
                BinaryColor::On,
                self.selection.stroke(1, 4),
            ))
            .draw(target)?;
        Label::new(self.title, TextRole::ControlLabel)
            .at(self.top_left + Point::new(24, 22))
            .draw(target)?;
        Label::new(self.detail, TextRole::Metadata)
            .at(self.top_left + Point::new(24, 55))
            .draw(target)
    }
}

#[derive(Clone, Copy)]
pub struct SettingsRow<'a> {
    label: &'a str,
    value: Option<&'a str>,
    selection: Selection,
    top_left: Point,
}

impl<'a> SettingsRow<'a> {
    pub const fn new(label: &'a str, value: Option<&'a str>, selection: Selection) -> Self {
        Self {
            label,
            value,
            selection,
            top_left: Point::zero(),
        }
    }
}

impl View for SettingsRow<'_> {
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

    fn bounds(&self) -> Rectangle {
        let height = if self.value.is_some() { 46 } else { 74 };
        Rectangle::new(self.top_left, Size::new(CONTENT_WIDTH, height))
    }
}

impl Drawable for SettingsRow<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        let (top, height, role, label_y) = match self.value {
            Some(_) => (self.top_left, 46, TextRole::Body, 14),
            None => (
                self.top_left + Point::new(0, 10),
                64,
                TextRole::ControlLabel,
                20,
            ),
        };
        Rectangle::new(top, Size::new(CONTENT_WIDTH, height))
            .into_styled(PrimitiveStyle::with_stroke(
                BinaryColor::On,
                self.selection.stroke(1, 3),
            ))
            .draw(target)?;
        Label::new(self.label, role)
            .at(top + Point::new(16, label_y))
            .draw(target)?;
        if let Some(value) = self.value {
            Label::new(value, TextRole::Body)
                .at(top + Point::new(312, 14))
                .draw(target)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct FileRow<'a> {
    name: &'a str,
    size: u32,
    kind: &'a str,
    selection: Selection,
    top_left: Point,
}

impl<'a> FileRow<'a> {
    pub const fn new(name: &'a str, size: u32, kind: &'a str, selection: Selection) -> Self {
        Self {
            name,
            size,
            kind,
            selection,
            top_left: Point::zero(),
        }
    }
}

impl View for FileRow<'_> {
    fn translate_impl(&mut self, by: Point) {
        self.top_left += by;
    }

    fn bounds(&self) -> Rectangle {
        Rectangle::new(self.top_left, Size::new(CONTENT_WIDTH, 62))
    }
}

impl Drawable for FileRow<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        self.bounds()
            .into_styled(PrimitiveStyle::with_stroke(
                BinaryColor::On,
                self.selection.stroke(1, 3),
            ))
            .draw(target)?;
        Label::new(self.name, TextRole::ControlLabel)
            .at(self.top_left + Point::new(14, 12))
            .clipped_to(Size::new(330, 18))
            .draw(target)?;
        let mut size = FixedText::<24>::new();
        write!(size, "{} KiB", self.size.div_ceil(1024)).ok();
        Label::new(size.as_str(), TextRole::Metadata)
            .at(self.top_left + Point::new(364, 20))
            .draw(target)?;
        Label::new(self.kind, TextRole::Metadata)
            .at(self.top_left + Point::new(14, 39))
            .draw(target)
    }
}

fn draw_battery<D>(target: &mut D, origin: Point, battery: BatteryStatus) -> Result<(), D::Error>
where
    D: DrawTarget<Color = BinaryColor>,
{
    Rectangle::new(origin + Point::new(374, 20), Size::new(27, 13))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(target)?;
    Rectangle::new(origin + Point::new(401, 24), Size::new(3, 5))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(target)?;

    let mut label = FixedText::<12>::new();
    match battery.level() {
        BatteryLevel::Unknown => label.write_str("--%").ok(),
        BatteryLevel::Percent(percent) => {
            let fill = usize::from(percent.get()) * 23 / 100;
            if fill > 0 && !battery.usb().is_connected() {
                Rectangle::new(origin + Point::new(376, 22), Size::new(fill as u32, 9))
                    .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                    .draw(target)?;
            }
            write!(label, "{}%", percent.get()).ok()
        }
    };
    Label::new(label.as_str(), TextRole::Metadata)
        .at(origin + Point::new(410, 22))
        .draw(target)?;
    if battery.usb().is_connected() {
        draw_external_power_symbol(target, origin)?;
    }
    Ok(())
}

fn draw_external_power_symbol<D>(target: &mut D, origin: Point) -> Result<(), D::Error>
where
    D: DrawTarget<Color = BinaryColor>,
{
    Rectangle::new(
        origin + Point::new(POWER_SYMBOL_X - 2, POWER_SYMBOL_Y),
        Size::new(9, POWER_SYMBOL_ROWS.len() as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
    .draw(target)?;
    target.draw_iter(
        POWER_SYMBOL_ROWS
            .into_iter()
            .enumerate()
            .flat_map(|(row, pixels)| {
                (0..5).filter_map(move |column| {
                    (pixels & (1 << (4 - column)) != 0).then_some(Pixel(
                        origin + Point::new(POWER_SYMBOL_X + column, POWER_SYMBOL_Y + row as i32),
                        BinaryColor::On,
                    ))
                })
            }),
    )
}
