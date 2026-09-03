use embedded_graphics::{Pixel, geometry::Point, pixelcolor::BinaryColor, prelude::DrawTarget};

#[derive(Clone, Copy)]
pub(crate) struct BitmapGlyph {
    bitmap_offset: usize,
    width: usize,
    height: usize,
    left: i32,
    top: i32,
    advance: usize,
}

impl BitmapGlyph {
    const fn new(
        bitmap_offset: usize,
        width: usize,
        height: usize,
        left: i32,
        top: i32,
        advance: usize,
    ) -> Self {
        Self {
            bitmap_offset,
            width,
            height,
            left,
            top,
            advance,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct BitmapFont {
    codepoints: &'static [u16],
    glyphs: &'static [BitmapGlyph],
    bitmap: &'static [u8],
    line_height: usize,
}

impl BitmapFont {
    const fn new(
        codepoints: &'static [u16],
        glyphs: &'static [BitmapGlyph],
        bitmap: &'static [u8],
        line_height: usize,
    ) -> Self {
        Self {
            codepoints,
            glyphs,
            bitmap,
            line_height,
        }
    }

    pub(crate) const fn line_height(self) -> usize {
        self.line_height
    }

    pub(crate) fn text_width(self, text: &str) -> usize {
        text.chars()
            .map(|character| self.glyph(character).advance)
            .sum()
    }

    pub(crate) fn character_width(self, character: char) -> usize {
        self.glyph(character).advance
    }

    pub(crate) fn draw<D>(self, text: &str, position: Point, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = BinaryColor>,
    {
        let mut cursor_x = position.x;
        for character in text.chars() {
            let glyph = self.glyph(character);
            let bitmap = self.bitmap;
            let pixels = (0..glyph.height).flat_map(|y| {
                (0..glyph.width).filter_map(move |x| {
                    let bit = y * glyph.width + x;
                    let byte = bitmap[glyph.bitmap_offset + bit / 8];
                    (byte & (0x80 >> (bit % 8)) != 0).then_some(Pixel(
                        Point::new(
                            cursor_x + glyph.left + x as i32,
                            position.y + glyph.top + y as i32,
                        ),
                        BinaryColor::On,
                    ))
                })
            });
            target.draw_iter(pixels)?;
            cursor_x += glyph.advance as i32;
        }
        Ok(())
    }

    fn glyph(self, character: char) -> &'static BitmapGlyph {
        let index = u16::try_from(character as u32)
            .ok()
            .and_then(|codepoint| self.codepoints.binary_search(&codepoint).ok())
            .unwrap_or_else(|| {
                self.codepoints
                    .binary_search(&u16::from(b'?'))
                    .expect("reader font contains its replacement glyph")
            });
        &self.glyphs[index]
    }
}

pub(crate) mod noto_serif;
