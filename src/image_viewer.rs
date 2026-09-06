use embedded_graphics::{
    Drawable,
    geometry::{Point, Size as GraphicsSize},
    pixelcolor::BinaryColor,
    prelude::Primitive,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};

use crate::{
    image::{MonochromeImage, Size},
    power::BatteryStatus,
    ui::{
        CONTENT_LEFT, FRAME_HEIGHT, FRAME_WIDTH, FrameTarget, chrome_style, draw_app_bar,
        draw_footer_rule,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageViewerRenderError {
    WrongFrameSize { actual: Size },
}

pub fn render_image_viewer(
    name: &str,
    selected_for_sleep: bool,
    battery: BatteryStatus,
    target: &mut MonochromeImage<'_>,
) -> Result<(), ImageViewerRenderError> {
    let expected = Size::new(FRAME_WIDTH, FRAME_HEIGHT).expect("viewer dimensions are non-zero");
    if target.size() != expected {
        return Err(ImageViewerRenderError::WrongFrameSize {
            actual: target.size(),
        });
    }

    let mut display = FrameTarget::new(target);
    Rectangle::new(Point::zero(), GraphicsSize::new(FRAME_WIDTH as u32, 76))
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
        .draw(&mut display)
        .ok();
    Rectangle::new(
        Point::new(0, 730),
        GraphicsSize::new(FRAME_WIDTH as u32, 70),
    )
    .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
    .draw(&mut display)
    .ok();
    draw_app_bar(target, name, battery);
    draw_footer_rule(target, 748);
    let footer = if selected_for_sleep {
        "SLEEP IMAGE SELECTED     BACK  FILES"
    } else {
        "CONFIRM  SELECT SLEEP     BACK  FILES"
    };
    Text::with_baseline(
        footer,
        Point::new(CONTENT_LEFT, 768),
        chrome_style(),
        Baseline::Top,
    )
    .draw(&mut FrameTarget::new(target))
    .ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::render_image_viewer;
    use crate::{
        image::{MonochromeImage, Size},
        power::BatteryStatus,
    };

    #[test]
    fn overlays_viewer_controls_without_replacing_the_image() {
        let mut bytes = std::vec![0; 48_000];
        let mut image = MonochromeImage::new(Size::new(480, 800).unwrap(), &mut bytes).unwrap();
        render_image_viewer("NICE.JPG", true, BatteryStatus::default(), &mut image).unwrap();
        assert!(image.pixel_is_black(0, 100));
        assert!(!image.pixel_is_black(0, 0));
    }
}
