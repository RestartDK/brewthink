use crate::image::{PackedBitmap, READER_DEPTH, Size};

pub const COVER_WIDTH: usize = 176;
pub const COVER_HEIGHT: usize = 264;
pub const COVER_BYTES: usize = COVER_WIDTH * COVER_HEIGHT / 8 * READER_DEPTH.bits();

pub fn bitmap(bytes: &[u8; COVER_BYTES]) -> PackedBitmap<'_> {
    PackedBitmap::new(cover_size(), READER_DEPTH, bytes)
        .expect("the packed cover buffer has the exact required length")
}

fn cover_size() -> Size {
    Size::new(COVER_WIDTH, COVER_HEIGHT).expect("cover dimensions are non-zero")
}
