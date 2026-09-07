use embedded_graphics::{
    Drawable,
    geometry::{Point, Size},
    pixelcolor::Gray8,
    prelude::{DrawTarget, Primitive},
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Icon {
    Book,
    Folder,
    Settings,
    Image,
    Font,
    TextSize,
    Spacing,
    Moon,
    Back,
    Confirm,
    Left,
    Right,
    Chapters,
    WifiOff,
}

impl Icon {
    pub const ALL: [Self; 14] = [
        Self::Book,
        Self::Folder,
        Self::Settings,
        Self::Image,
        Self::Font,
        Self::TextSize,
        Self::Spacing,
        Self::Moon,
        Self::Back,
        Self::Confirm,
        Self::Left,
        Self::Right,
        Self::Chapters,
        Self::WifiOff,
    ];

    pub fn draw<D>(self, target: &mut D, at: Point, color: Gray8) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Gray8>,
    {
        let stroke = PrimitiveStyle::with_stroke(color, 2);
        let paths: &[&[(i32, i32)]] = match self {
            Self::Book => &[&[
                (12, 5),
                (8, 3),
                (2, 3),
                (2, 19),
                (8, 19),
                (12, 21),
                (16, 19),
                (22, 19),
                (22, 3),
                (16, 3),
                (12, 5),
                (12, 21),
            ]],
            Self::Folder => &[
                &[(2, 19), (2, 4), (9, 4), (12, 7), (22, 7), (22, 19), (2, 19)],
                &[(2, 10), (22, 10)],
            ],
            Self::Settings => &[
                &[(2, 6), (7, 6)],
                &[(12, 6), (22, 6)],
                &[(9, 3), (9, 9)],
                &[(2, 12), (14, 12)],
                &[(19, 12), (22, 12)],
                &[(16, 9), (16, 15)],
                &[(2, 18), (5, 18)],
                &[(10, 18), (22, 18)],
                &[(7, 15), (7, 21)],
            ],
            Self::Image => &[
                &[(2, 3), (22, 3), (22, 21), (2, 21), (2, 3)],
                &[(3, 18), (9, 12), (13, 16), (17, 12), (22, 17)],
            ],
            Self::Font => &[
                &[(3, 7), (3, 3), (21, 3), (21, 7)],
                &[(12, 3), (12, 21)],
                &[(7, 21), (17, 21)],
            ],
            Self::TextSize => &[
                &[(2, 20), (8, 3), (14, 20)],
                &[(5, 13), (11, 13)],
                &[(16, 20), (19, 11), (22, 20)],
                &[(17, 17), (21, 17)],
            ],
            Self::Spacing => &[
                &[(11, 4), (22, 4)],
                &[(11, 12), (22, 12)],
                &[(11, 20), (22, 20)],
                &[(4, 3), (4, 21)],
                &[(1, 6), (4, 3), (7, 6)],
                &[(1, 18), (4, 21), (7, 18)],
            ],
            Self::Moon => &[&[
                (13, 2),
                (7, 4),
                (3, 9),
                (3, 15),
                (7, 20),
                (13, 22),
                (19, 19),
                (22, 14),
                (16, 15),
                (11, 12),
                (10, 7),
                (13, 2),
            ]],
            Self::Back => &[
                &[(10, 4), (3, 11), (10, 18)],
                &[(3, 11), (20, 11), (20, 20)],
            ],
            Self::Confirm => &[&[(4, 12), (9, 17), (20, 6)]],
            Self::Left => &[&[(15, 4), (7, 12), (15, 20)]],
            Self::Right => &[&[(9, 4), (17, 12), (9, 20)]],
            Self::Chapters => &[
                &[(8, 5), (22, 5)],
                &[(8, 12), (22, 12)],
                &[(8, 19), (22, 19)],
                &[(2, 5), (3, 5)],
                &[(2, 12), (3, 12)],
                &[(2, 19), (3, 19)],
            ],
            Self::WifiOff => &[
                &[(2, 7), (6, 4), (12, 3), (18, 4), (22, 7)],
                &[(5, 11), (8, 9), (12, 8), (16, 9), (19, 11)],
                &[(8, 15), (12, 13), (16, 15)],
                &[(2, 2), (22, 22)],
            ],
        };
        for path in paths {
            for segment in path.windows(2) {
                Line::new(
                    at + Point::new(segment[0].0, segment[0].1),
                    at + Point::new(segment[1].0, segment[1].1),
                )
                .into_styled(stroke)
                .draw(target)?;
            }
        }
        if self == Self::Image {
            Circle::new(at + Point::new(6, 6), 4)
                .into_styled(stroke)
                .draw(target)?;
        }
        if self == Self::WifiOff {
            Rectangle::new(at + Point::new(11, 18), Size::new(3, 3))
                .into_styled(PrimitiveStyle::with_fill(color))
                .draw(target)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use crate::{
        image::{PackedImage, READER_DEPTH, Size as ImageSize},
        ui::{CHROME_INK, FrameTarget},
    };
    use embedded_graphics::pixelcolor::GrayColor;

    #[test]
    fn every_icon_fits_its_grid_and_uses_the_requested_tone() {
        for icon in Icon::ALL {
            let size = ImageSize::new(64, 64).unwrap();
            let mut bytes = std::vec![0xff; READER_DEPTH.byte_len(size).unwrap()];
            let mut image = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
            icon.draw(
                &mut FrameTarget::new(&mut image),
                Point::new(20, 20),
                CHROME_INK,
            )
            .unwrap();
            let mut count = 0;
            for y in 0..64 {
                for x in 0..64 {
                    if image.luma(x, y) == CHROME_INK.luma() {
                        assert!(
                            (20..44).contains(&x) && (20..44).contains(&y),
                            "{icon:?} escaped its grid at {x}, {y}"
                        );
                        count += 1;
                    }
                }
            }
            assert!(count > 8, "{icon:?} is empty");
        }
    }
}
