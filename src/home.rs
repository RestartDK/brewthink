use embedded_graphics::{Drawable, geometry::Point};
use embedded_layout::View;

use crate::{
    app::{HomeItem, HomeState},
    image::{MonochromeImage, Size},
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
    target: &mut MonochromeImage<'_>,
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

    use super::render_home;
    use crate::{
        app::HomeState,
        image::{MonochromeImage, Size},
        input::UsbState,
        power::BatteryStatus,
    };

    #[test]
    fn renders_home_into_the_exact_x4_frame() {
        let mut bytes = std::vec![0xFF; 480 * 800 / 8];
        let mut image = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();
        render_home(
            HomeState::new(),
            BatteryStatus::from_percent(82, UsbState::Disconnected),
            &mut image,
        )
        .unwrap();
        assert!(!image.pixel_is_black(18, 58));
        assert!(image.pixel_is_black(24, 120));
        assert!(!image.pixel_is_black(24, 200));
    }
}
