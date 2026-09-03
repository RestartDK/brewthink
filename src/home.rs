use embedded_graphics::{
    Drawable,
    geometry::{Point, Size as GraphicsSize},
    pixelcolor::BinaryColor,
    prelude::Primitive,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};

use crate::{
    app::{HomeItem, HomeState},
    image::{MonochromeImage, Size},
    power::BatteryStatus,
    ui::{
        CONTENT_LEFT, CONTENT_WIDTH, FRAME_HEIGHT, FRAME_WIDTH, FrameTarget, brand_style,
        chrome_style, draw_app_bar, draw_footer_rule,
    },
};

const ROW_TOPS: [i32; 3] = [150, 282, 414];

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
    draw_app_bar(target, "HOME", battery);
    let mut display = FrameTarget::new(target);
    Text::with_baseline(
        "CHOOSE WHERE TO GO",
        Point::new(CONTENT_LEFT, 92),
        chrome_style(),
        Baseline::Top,
    )
    .draw(&mut display)
    .ok();

    for (index, item) in HomeItem::ALL.into_iter().enumerate() {
        draw_row(
            &mut display,
            item,
            ROW_TOPS[index],
            item == state.selected(),
        );
    }

    draw_footer_rule(target, 748);
    Text::with_baseline(
        "UP/DOWN  MOVE     CONFIRM  OPEN",
        Point::new(CONTENT_LEFT, 768),
        chrome_style(),
        Baseline::Top,
    )
    .draw(&mut FrameTarget::new(target))
    .ok();
    Ok(())
}

fn draw_row(display: &mut FrameTarget<'_, '_>, item: HomeItem, top: i32, selected: bool) {
    Rectangle::new(
        Point::new(CONTENT_LEFT, top),
        GraphicsSize::new(CONTENT_WIDTH, 92),
    )
    .into_styled(PrimitiveStyle::with_stroke(
        BinaryColor::On,
        if selected { 4 } else { 1 },
    ))
    .draw(display)
    .ok();

    Text::with_baseline(
        item.label(),
        Point::new(42, top + 22),
        brand_style(),
        Baseline::Top,
    )
    .draw(display)
    .ok();
    let detail = match item {
        HomeItem::Books => "COVERS AND READING PROGRESS",
        HomeItem::Files => "EPUB FILES ON MICROSD",
        HomeItem::Settings => "FONT, SIZE, AND SPACING",
    };
    Text::with_baseline(
        detail,
        Point::new(42, top + 55),
        chrome_style(),
        Baseline::Top,
    )
    .draw(display)
    .ok();
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
        assert!(image.pixel_is_black(18, 58));
        assert!(image.pixel_is_black(18, 150));
    }
}
