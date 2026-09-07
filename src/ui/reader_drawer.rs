use core::fmt::Write;

use embedded_graphics::{
    Drawable,
    geometry::{Point, Size},
    pixelcolor::Gray8,
    prelude::{DrawTarget, Primitive},
    primitives::{Circle, PrimitiveStyle, Rectangle},
};

use super::{
    AppBar, CHROME_PAPER, CommandBar, DrawerSurface, FixedText, Icon, Label, Selection, TextRole,
    components::draw_selection, text_width,
};
use crate::{
    app::{ReaderControl, ReaderDrawer},
    power::BatteryStatus,
};

pub fn draw_reader_drawer<D>(
    target: &mut D,
    drawer: ReaderDrawer,
    book_title: &str,
    chapter_title: &str,
    battery: BatteryStatus,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = Gray8>,
{
    Rectangle::new(Point::zero(), Size::new(480, 58))
        .into_styled(PrimitiveStyle::with_fill(CHROME_PAPER))
        .draw(target)?;
    AppBar::new("Reading", battery).draw(target)?;
    DrawerSurface::new(304).draw(target)?;
    Label::new(book_title, TextRole::Heading)
        .at(Point::new(30, 336))
        .clipped_to(Size::new(420, 34))
        .draw(target)?;
    Label::new("Choose a control with the side buttons", TextRole::Metadata)
        .at(Point::new(30, 371))
        .clipped_to(Size::new(420, 22))
        .draw(target)?;

    let location = drawer.session().location();
    for (index, item) in ReaderControl::ALL.into_iter().enumerate() {
        let top = match index {
            0 => 400,
            1 => 484,
            _ => 562 + (index as i32 - 2) * 48,
        };
        let height = match item {
            ReaderControl::Position => 78,
            ReaderControl::Chapter => 72,
            _ => 44,
        };
        let color = draw_selection(
            target,
            Rectangle::new(Point::new(18, top), Size::new(444, height)),
            Selection::from_selected(item == drawer.selected()),
        )?;
        let icon = match item {
            ReaderControl::Position => Icon::Book,
            ReaderControl::Chapter => Icon::Chapters,
            ReaderControl::Font => Icon::Font,
            ReaderControl::Size => Icon::TextSize,
            ReaderControl::Spacing => Icon::Spacing,
        };
        icon.draw(target, Point::new(32, top + 10), color)?;
        Label::new(item.label(), TextRole::ControlLabel)
            .color(color)
            .at(Point::new(72, top + 7))
            .draw(target)?;
        let mut value = FixedText::<48>::new();
        match item {
            ReaderControl::Position => write!(value, "{}%", drawer.position().percent()).ok(),
            ReaderControl::Chapter => write!(
                value,
                "{} / {}",
                drawer.chapter() + 1,
                location.spine_count()
            )
            .ok(),
            ReaderControl::Font => value.write_str(drawer.preferences().font().label()).ok(),
            ReaderControl::Size => value.write_str(drawer.preferences().size().label()).ok(),
            ReaderControl::Spacing => value.write_str(drawer.preferences().spacing().label()).ok(),
        };
        Label::new(value.as_str(), TextRole::Body)
            .color(color)
            .at(Point::new(
                426 - text_width(TextRole::Body, value.as_str()) as i32,
                top + 11,
            ))
            .draw(target)?;
        if item == ReaderControl::Chapter {
            Label::new(chapter_title, TextRole::Body)
                .color(color)
                .at(Point::new(72, top + 35))
                .clipped_to(Size::new(358, 28))
                .draw(target)?;
        }
        if item == ReaderControl::Position {
            Rectangle::new(Point::new(74, top + 56), Size::new(346, 2))
                .into_styled(PrimitiveStyle::with_fill(color))
                .draw(target)?;
            let x = 74 + drawer.position().permille() * 346 / 1000;
            Circle::new(Point::new(x as i32 - 5, top + 51), 12)
                .into_styled(PrimitiveStyle::with_fill(color))
                .draw(target)?;
        }
    }
    let action = match drawer.selected() {
        ReaderControl::Position | ReaderControl::Chapter => "Go to",
        ReaderControl::Font | ReaderControl::Size | ReaderControl::Spacing => "Apply",
    };
    Label::new("Book position: approximate, by chapter", TextRole::Metadata)
        .at(Point::new(30, 708))
        .draw(target)?;
    CommandBar::new(["Cancel", action, "Decrease", "Increase"]).draw(target)
}
