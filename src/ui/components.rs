use core::fmt::Write;

use embedded_graphics::{
    Drawable, Pixel,
    draw_target::DrawTargetExt,
    geometry::{Dimensions, Point, Size},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Primitive},
    primitives::{PrimitiveStyle, Rectangle, RoundedRectangle},
    text::{Baseline, Text},
};
use embedded_layout::View;

use crate::{
    app::SettingsItem,
    files::FileKind,
    power::{BatteryLevel, BatteryStatus},
};

use super::{
    APP_BAR_RULE_Y, CONTENT_LEFT, CONTENT_WIDTH, FOOTER_RULE_Y, FOOTER_TEXT_Y, FRAME_WIDTH,
    FixedText, Icon, TextRole, text_style,
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
    color: BinaryColor,
}

impl<'a> Label<'a> {
    pub const fn new(text: &'a str, role: TextRole) -> Self {
        Self {
            text,
            role,
            top_left: Point::zero(),
            clip: None,
            color: BinaryColor::On,
        }
    }

    pub const fn at(mut self, top_left: Point) -> Self {
        self.top_left = top_left;
        self
    }

    pub const fn color(mut self, color: BinaryColor) -> Self {
        self.color = color;
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
        let mut style = text_style(self.role);
        style.text_color = Some(self.color);
        let text = Text::with_baseline(self.text, self.top_left, style, Baseline::Top);
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
        Label::new(self.section, TextRole::Section)
            .at(self.top_left + Point::new(CONTENT_LEFT, 20))
            .clipped_to(Size::new(302, 22))
            .draw(target)?;
        Icon::WifiOff.draw(target, self.top_left + Point::new(334, 15), BinaryColor::On)?;
        draw_battery(target, self.top_left, self.battery)
    }
}

#[derive(Clone, Copy)]
pub struct CommandBar<'a> {
    actions: [&'a str; 4],
    left: i32,
    width: u32,
    rule_y: i32,
    text_y: i32,
}

impl<'a> CommandBar<'a> {
    pub const fn new(actions: [&'a str; 4]) -> Self {
        Self {
            actions,
            left: CONTENT_LEFT,
            width: CONTENT_WIDTH,
            rule_y: FOOTER_RULE_Y,
            text_y: FOOTER_TEXT_Y,
        }
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
        let icons = [Icon::Back, Icon::Confirm, Icon::Left, Icon::Right];
        let column = self.width / 4;
        for (index, (icon, action)) in icons.into_iter().zip(self.actions).enumerate() {
            let center = self.left + column as i32 * index as i32 + column as i32 / 2;
            icon.draw(
                target,
                Point::new(center - 12, self.rule_y + 8),
                BinaryColor::On,
            )?;
            Label::new(action, TextRole::CommandHint)
                .at(Point::new(center - action.len() as i32 * 3, self.text_y))
                .clipped_to(Size::new(column, 10))
                .draw(target)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct MenuRow<'a> {
    title: &'a str,
    icon: Icon,
    selection: Selection,
    top_left: Point,
}

impl<'a> MenuRow<'a> {
    pub const fn new(title: &'a str, icon: Icon, selection: Selection) -> Self {
        Self {
            title,
            icon,
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
        Rectangle::new(self.top_left, Size::new(CONTENT_WIDTH, 76))
    }
}

impl Drawable for MenuRow<'_> {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        let color = draw_selection(target, self.bounds(), self.selection)?;
        self.icon
            .draw(target, self.top_left + Point::new(20, 26), color)?;
        Label::new(self.title, TextRole::ControlLabel)
            .color(color)
            .at(self.top_left + Point::new(68, 29))
            .draw(target)
    }
}

#[derive(Clone, Copy)]
pub struct SettingsRow<'a> {
    item: SettingsItem,
    value: Option<&'a str>,
    selection: Selection,
    top_left: Point,
}

impl<'a> SettingsRow<'a> {
    pub const fn new(item: SettingsItem, value: Option<&'a str>, selection: Selection) -> Self {
        Self {
            item,
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
            Some(_) => (self.top_left, 46, TextRole::ControlLabel, 14),
            None => (
                self.top_left + Point::new(0, 10),
                64,
                TextRole::ControlLabel,
                20,
            ),
        };
        let color = draw_selection(
            target,
            Rectangle::new(top, Size::new(CONTENT_WIDTH, height)),
            self.selection,
        )?;
        let icon = match self.item {
            SettingsItem::Font => Icon::Font,
            SettingsItem::Size => Icon::TextSize,
            SettingsItem::Spacing => Icon::Spacing,
            SettingsItem::SleepScreen => Icon::Moon,
            SettingsItem::Apply => Icon::Confirm,
        };
        icon.draw(target, top + Point::new(20, label_y - 3), color)?;
        Label::new(self.item.label(), role)
            .color(color)
            .at(top + Point::new(68, label_y))
            .draw(target)?;
        if let Some(value) = self.value {
            Label::new(value, TextRole::Body)
                .color(color)
                .at(top + Point::new(268, 14))
                .draw(target)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct FileRow<'a> {
    name: &'a str,
    size: u32,
    kind: FileKind,
    selection: Selection,
    top_left: Point,
}

impl<'a> FileRow<'a> {
    pub const fn new(name: &'a str, size: u32, kind: FileKind, selection: Selection) -> Self {
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
        let color = draw_selection(target, self.bounds(), self.selection)?;
        let icon = match self.kind {
            FileKind::Epub => Icon::Book,
            FileKind::Jpeg | FileKind::Png => Icon::Image,
        };
        icon.draw(target, self.top_left + Point::new(16, 19), color)?;
        Label::new(self.name, TextRole::ControlLabel)
            .color(color)
            .at(self.top_left + Point::new(58, 12))
            .clipped_to(Size::new(370, 18))
            .draw(target)?;
        let mut size = FixedText::<32>::new();
        write!(
            size,
            "{}  {} KiB",
            self.kind.label(),
            self.size.div_ceil(1024)
        )
        .ok();
        Label::new(size.as_str(), TextRole::Metadata)
            .color(color)
            .at(self.top_left + Point::new(58, 38))
            .draw(target)
    }
}

pub(super) fn draw_selection<D>(
    target: &mut D,
    bounds: Rectangle,
    selection: Selection,
) -> Result<BinaryColor, D::Error>
where
    D: DrawTarget<Color = BinaryColor>,
{
    if selection == Selection::Selected {
        RoundedRectangle::with_equal_corners(bounds, Size::new(8, 8))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(target)?;
        Ok(BinaryColor::Off)
    } else {
        Ok(BinaryColor::On)
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
