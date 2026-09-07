use embedded_graphics::{
    Drawable, Pixel,
    geometry::{Point, Size},
    pixelcolor::Gray8,
    prelude::{DrawTarget, Primitive},
    primitives::{PrimitiveStyleBuilder, Rectangle, RoundedRectangle},
};

use super::{APP_BAR_RULE_Y, CHROME_INK, CHROME_PAPER, FRAME_HEIGHT, FRAME_WIDTH, PANEL_CORNERS};

pub struct DrawerSurface {
    top: i32,
}

impl DrawerSurface {
    pub const fn new(top: i32) -> Self {
        Self { top }
    }
}

impl Drawable for DrawerSurface {
    type Color = Gray8;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Gray8>,
    {
        target.draw_iter((APP_BAR_RULE_Y..FRAME_HEIGHT as i32).flat_map(|y| {
            ((y & 1)..FRAME_WIDTH as i32)
                .step_by(2)
                .map(move |x| Pixel(Point::new(x, y), CHROME_PAPER))
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
                    .fill_color(CHROME_PAPER)
                    .stroke_color(CHROME_INK)
                    .stroke_width(2)
                    .build(),
            )
            .draw(target)
    }
}
