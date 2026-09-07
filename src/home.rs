use embedded_graphics::{Drawable, geometry::Point};
use embedded_layout::View;

use crate::{
    app::{HomeItem, HomeState},
    image::{PackedImage, Size},
    power::BatteryStatus,
    ui::{
        AppBar, CONTENT_LEFT, CommandBar, FRAME_HEIGHT, FRAME_WIDTH, FrameTarget, Icon, MenuRow,
        Selection, ui, ui_column,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HomeRenderError {
    WrongFrameSize { actual: Size },
}

pub fn render_home(
    state: HomeState,
    battery: BatteryStatus,
    target: &mut PackedImage<'_>,
) -> Result<(), HomeRenderError> {
    let expected =
        Size::new(FRAME_WIDTH, FRAME_HEIGHT).expect("home frame dimensions are non-zero");
    if target.size() != expected {
        return Err(HomeRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }

    target.clear_white();
    let selected = state.selected();
    let rows = ui_column!(
        4;
        menu_row(HomeItem::Books, selected),
        menu_row(HomeItem::Files, selected),
        menu_row(HomeItem::Settings, selected),
    )
    .translate(Point::new(CONTENT_LEFT, 106));
    let screen = ui!(
        AppBar::new("Home", battery),
        rows,
        CommandBar::new(["", "Open", "Previous", "Next"]),
    );
    screen.draw(&mut FrameTarget::new(target)).ok();
    Ok(())
}

fn menu_row(item: HomeItem, selected: HomeItem) -> MenuRow<'static> {
    let icon = match item {
        HomeItem::Books => Icon::Book,
        HomeItem::Files => Icon::Folder,
        HomeItem::Settings => Icon::Settings,
    };
    MenuRow::new(
        item.label(),
        icon,
        Selection::from_selected(item == selected),
    )
}

#[cfg(test)]
mod tests {
    extern crate std;

    use embedded_graphics::pixelcolor::GrayColor;

    use super::render_home;
    use crate::{
        app::{HomeItem, HomeState},
        image::{PackedImage, READER_DEPTH, Size},
        input::UsbState,
        power::BatteryStatus,
        ui::SELECTION_BACKGROUND,
    };

    #[test]
    fn every_home_row_uses_gray_selection_with_black_foreground_and_outline() {
        let size = Size::new(480, 800).unwrap();
        let battery = BatteryStatus::from_percent(82, UsbState::Disconnected);
        let tops = [106, 186, 266];
        let foreground = [(50, 137), (40, 231), (40, 298)];

        for (index, item) in HomeItem::ALL.into_iter().enumerate() {
            let mut bytes = std::vec![0xFF; READER_DEPTH.byte_len(size).unwrap()];
            let mut image = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
            render_home(HomeState::with_selected(item), battery, &mut image).unwrap();

            assert_eq!(
                image.luma(450, tops[index] + 38),
                SELECTION_BACKGROUND.luma(),
                "home row {index} lost its selection fill"
            );
            assert_eq!(
                image.luma(240, tops[index]),
                0,
                "home row {index} lost its outline"
            );
            assert_eq!(
                image.luma(foreground[index].0, foreground[index].1),
                0,
                "home row {index} lost its black icon"
            );
        }
    }
}
