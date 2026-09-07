use embedded_graphics::{
    Drawable, Pixel,
    geometry::{Point, Size},
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, Primitive},
    primitives::{PrimitiveStyleBuilder, Rectangle, RoundedRectangle},
};

use super::{APP_BAR_RULE_Y, FRAME_HEIGHT, FRAME_WIDTH, PANEL_CORNERS};

pub struct DrawerSurface {
    top: i32,
}

impl DrawerSurface {
    pub const fn new(top: i32) -> Self {
        Self { top }
    }
}

impl Drawable for DrawerSurface {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = BinaryColor>,
    {
        target.draw_iter((APP_BAR_RULE_Y..FRAME_HEIGHT as i32).flat_map(|y| {
            ((y & 1)..FRAME_WIDTH as i32)
                .step_by(2)
                .map(move |x| Pixel(Point::new(x, y), BinaryColor::Off))
        }))?;
        let bounds = Rectangle::new(
            Point::new(8, self.top),
            Size::new(
                FRAME_WIDTH as u32 - 16,
                (FRAME_HEIGHT as i32 - 8 - self.top) as u32,
            ),
        );
        RoundedRectangle::with_equal_corners(bounds, PANEL_CORNERS)
            .into_styled(
                PrimitiveStyleBuilder::new()
                    .fill_color(BinaryColor::Off)
                    .stroke_color(BinaryColor::On)
                    .stroke_width(2)
                    .build(),
            )
            .draw(target)
    }
}
