use core::fmt::Write;

use embedded_graphics::{
    Drawable,
    geometry::{Point, Size},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Primitive},
    primitives::{Circle, PrimitiveStyle, Rectangle},
};

use super::{
    AppBar, CommandBar, FixedText, Icon, Label, Selection, TextRole, components::draw_selection,
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
    D: DrawTarget<Color = BinaryColor>,
{
    Rectangle::new(Point::zero(), Size::new(480, 58))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
        .draw(target)?;
    AppBar::new("Reading", battery).draw(target)?;
    Rectangle::new(Point::new(0, 352), Size::new(480, 448))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
        .draw(target)?;
    Rectangle::new(Point::new(18, 352), Size::new(444, 2))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(target)?;
    Label::new(book_title, TextRole::Heading)
        .at(Point::new(30, 372))
        .clipped_to(Size::new(420, 20))
        .draw(target)?;
    Label::new(chapter_title, TextRole::Metadata)
        .at(Point::new(30, 401))
        .clipped_to(Size::new(420, 12))
        .draw(target)?;

    let location = drawer.session().location();
    for (index, item) in ReaderControl::ALL.into_iter().enumerate() {
        let top = if index == 0 {
            430
        } else {
            514 + (index as i32 - 1) * 48
        };
        let height = if item == ReaderControl::Page { 78 } else { 44 };
        let color = draw_selection(
            target,
            Rectangle::new(Point::new(18, top), Size::new(444, height)),
            Selection::from_selected(item == drawer.selected()),
        )?;
        let icon = match item {
            ReaderControl::Page => Icon::Book,
            ReaderControl::Chapter => Icon::Chapters,
            ReaderControl::Font => Icon::Font,
            ReaderControl::Size => Icon::TextSize,
            ReaderControl::Spacing => Icon::Spacing,
        };
        icon.draw(target, Point::new(32, top + 10), color)?;
        Label::new(item.label(), TextRole::ControlLabel)
            .color(color)
            .at(Point::new(72, top + 13))
            .draw(target)?;
        let mut value = FixedText::<48>::new();
        match item {
            ReaderControl::Page => {
                write!(value, "{} / {}", drawer.page() + 1, location.page_count()).ok()
            }
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
            .at(Point::new(426 - value.as_str().len() as i32 * 6, top + 17))
            .draw(target)?;
        if item == ReaderControl::Page {
            Rectangle::new(Point::new(74, top + 56), Size::new(346, 2))
                .into_styled(PrimitiveStyle::with_fill(color))
                .draw(target)?;
            let x = 74 + drawer.page() * 346 / location.page_count().saturating_sub(1).max(1);
            Circle::new(Point::new(x as i32 - 5, top + 51), 12)
                .into_styled(PrimitiveStyle::with_fill(color))
                .draw(target)?;
        }
    }
    let action = match drawer.selected() {
        ReaderControl::Page | ReaderControl::Chapter => "Go to",
        ReaderControl::Font | ReaderControl::Size | ReaderControl::Spacing => "Apply",
    };
    Label::new("Side buttons: choose a row", TextRole::Metadata)
        .at(Point::new(30, 716))
        .draw(target)?;
    CommandBar::new(["Cancel", action, "Decrease", "Increase"]).draw(target)
}
