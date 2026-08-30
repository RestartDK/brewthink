use super::ssd1677::{
    FRAME_BYTES as PANEL_FRAME_BYTES, HEIGHT as PANEL_HEIGHT, ROW_BYTES as PANEL_ROW_BYTES,
    WIDTH as PANEL_WIDTH,
};

pub const FRAME_BYTES: usize = PANEL_FRAME_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rotation {
    Degrees0,
    Degrees90,
    Degrees180,
    Degrees270,
}

impl Rotation {
    pub const fn logical_width(self) -> usize {
        match self {
            Self::Degrees0 | Self::Degrees180 => PANEL_WIDTH,
            Self::Degrees90 | Self::Degrees270 => PANEL_HEIGHT,
        }
    }

    pub const fn logical_height(self) -> usize {
        match self {
            Self::Degrees0 | Self::Degrees180 => PANEL_HEIGHT,
            Self::Degrees90 | Self::Degrees270 => PANEL_WIDTH,
        }
    }

    pub const fn degrees(self) -> u16 {
        match self {
            Self::Degrees0 => 0,
            Self::Degrees90 => 90,
            Self::Degrees180 => 180,
            Self::Degrees270 => 270,
        }
    }

    const fn panel_to_logical(self, panel_x: usize, panel_y: usize) -> (usize, usize) {
        match self {
            Self::Degrees0 => (panel_x, panel_y),
            Self::Degrees90 => (panel_y, PANEL_WIDTH - 1 - panel_x),
            Self::Degrees180 => (PANEL_WIDTH - 1 - panel_x, PANEL_HEIGHT - 1 - panel_y),
            Self::Degrees270 => (PANEL_HEIGHT - 1 - panel_y, panel_x),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidFrameLength { expected: usize, actual: usize },
}

#[derive(Clone, Copy)]
pub struct Frame<'a> {
    bytes: &'a [u8],
    rotation: Rotation,
}

impl<'a> Frame<'a> {
    pub fn new(bytes: &'a [u8], rotation: Rotation) -> Result<Self, Error> {
        if bytes.len() != FRAME_BYTES {
            return Err(Error::InvalidFrameLength {
                expected: FRAME_BYTES,
                actual: bytes.len(),
            });
        }

        Ok(Self { bytes, rotation })
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub const fn rotation(&self) -> Rotation {
        self.rotation
    }

    pub const fn width(&self) -> usize {
        self.rotation.logical_width()
    }

    pub const fn height(&self) -> usize {
        self.rotation.logical_height()
    }

    pub(crate) fn fill_panel_chunk(&self, offset: usize, output: &mut [u8]) {
        let row_bytes = self.width() / 8;
        fill_panel_chunk_with(self.rotation, offset, output, |x, y| {
            self.bytes[y * row_bytes + x / 8] & (0x80 >> (x % 8)) == 0
        });
    }
}

pub(crate) fn fill_panel_chunk_with<F>(
    rotation: Rotation,
    offset: usize,
    output: &mut [u8],
    mut is_black: F,
) where
    F: FnMut(usize, usize) -> bool,
{
    debug_assert!(offset + output.len() <= PANEL_FRAME_BYTES);

    for (index, byte) in output.iter_mut().enumerate() {
        let position = offset + index;
        let panel_y = position / PANEL_ROW_BYTES;
        let first_panel_x = position % PANEL_ROW_BYTES * 8;
        let mut pixels = 0xFF;

        for bit in 0..8 {
            let panel_x = first_panel_x + bit;
            let (logical_x, logical_y) = rotation.panel_to_logical(panel_x, panel_y);
            if is_black(logical_x, logical_y) {
                pixels &= !(0x80 >> bit);
            }
        }

        *byte = pixels;
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec;

    use super::{Error, FRAME_BYTES, Frame, Rotation};
    use crate::display::ssd1677::{FRAME_BYTES as PANEL_FRAME_BYTES, ROW_BYTES as PANEL_ROW_BYTES};

    #[test]
    fn frame_requires_exactly_48000_bytes() {
        assert!(matches!(
            Frame::new(&[0; 32], Rotation::Degrees270),
            Err(Error::InvalidFrameLength {
                expected: FRAME_BYTES,
                actual: 32,
            })
        ));
        assert!(Frame::new(&vec![0xFF; FRAME_BYTES], Rotation::Degrees270).is_ok());
    }

    #[test]
    fn rotation_exposes_logical_dimensions() {
        assert_eq!(dimensions(Rotation::Degrees0), (800, 480));
        assert_eq!(dimensions(Rotation::Degrees90), (480, 800));
        assert_eq!(dimensions(Rotation::Degrees180), (800, 480));
        assert_eq!(dimensions(Rotation::Degrees270), (480, 800));
    }

    #[test]
    fn all_rotations_map_logical_corners_to_panel_ram() {
        let cases = [
            (Rotation::Degrees0, [(0, 0), (799, 0), (0, 479), (799, 479)]),
            (
                Rotation::Degrees90,
                [(799, 0), (799, 479), (0, 0), (0, 479)],
            ),
            (
                Rotation::Degrees180,
                [(799, 479), (0, 479), (799, 0), (0, 0)],
            ),
            (
                Rotation::Degrees270,
                [(0, 479), (0, 0), (799, 479), (799, 0)],
            ),
        ];

        for (rotation, expected_panel_corners) in cases {
            let width = rotation.logical_width();
            let height = rotation.logical_height();
            let logical_corners = [
                (0, 0),
                (width - 1, 0),
                (0, height - 1),
                (width - 1, height - 1),
            ];

            for (logical, expected_panel) in logical_corners.into_iter().zip(expected_panel_corners)
            {
                let mut bytes = vec![0xFF; FRAME_BYTES];
                set_black(&mut bytes, width, logical.0, logical.1);
                let frame = Frame::new(&bytes, rotation).unwrap();
                let mut transformed = vec![0xFF; PANEL_FRAME_BYTES];
                frame.fill_panel_chunk(0, &mut transformed);

                assert_eq!(black_pixels(&transformed), vec![expected_panel]);
            }
        }
    }

    #[test]
    fn rotation_transform_is_independent_of_transfer_chunks() {
        let mut bytes = vec![0xFF; FRAME_BYTES];
        for index in (0..FRAME_BYTES).step_by(97) {
            bytes[index] = (index % 251) as u8;
        }
        let frame = Frame::new(&bytes, Rotation::Degrees270).unwrap();
        let mut whole = vec![0; PANEL_FRAME_BYTES];
        let mut chunked = vec![0; PANEL_FRAME_BYTES];

        frame.fill_panel_chunk(0, &mut whole);
        for offset in (0..PANEL_FRAME_BYTES).step_by(257) {
            let end = (offset + 257).min(PANEL_FRAME_BYTES);
            frame.fill_panel_chunk(offset, &mut chunked[offset..end]);
        }

        assert_eq!(chunked, whole);
    }

    fn dimensions(rotation: Rotation) -> (usize, usize) {
        (rotation.logical_width(), rotation.logical_height())
    }

    fn set_black(frame: &mut [u8], width: usize, x: usize, y: usize) {
        let row_bytes = width / 8;
        frame[y * row_bytes + x / 8] &= !(0x80 >> (x % 8));
    }

    fn black_pixels(panel: &[u8]) -> std::vec::Vec<(usize, usize)> {
        let mut pixels = vec![];
        for y in 0..480 {
            for x in 0..800 {
                if panel[y * PANEL_ROW_BYTES + x / 8] & (0x80 >> (x % 8)) == 0 {
                    pixels.push((x, y));
                }
            }
        }
        pixels
    }
}
