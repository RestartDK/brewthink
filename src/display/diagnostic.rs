use super::{
    framebuffer::{Rotation, fill_panel_chunk_with},
    ssd1677::ROW_BYTES,
};

const CHECKER_TILE_BYTES: usize = 5;
const CHECKER_TILE_ROWS: usize = 40;
const BORDER: usize = 4;
const GLYPH_SCALE: usize = 6;
const GLYPH_WIDTH: usize = 5;
const GLYPH_HEIGHT: usize = 7;
const LABEL_GAP: usize = GLYPH_SCALE;
const LABEL_WIDTH: usize = GLYPH_WIDTH * GLYPH_SCALE * 2 + LABEL_GAP;
const LABEL_HEIGHT: usize = GLYPH_HEIGHT * GLYPH_SCALE;
const LABEL_MARGIN: usize = 20;

pub fn fill_checkerboard(offset: usize, output: &mut [u8]) {
    for (index, byte) in output.iter_mut().enumerate() {
        let position = offset + index;
        let byte_x = position % ROW_BYTES;
        let y = position / ROW_BYTES;
        let black = (byte_x / CHECKER_TILE_BYTES + y / CHECKER_TILE_ROWS).is_multiple_of(2);
        *byte = if black { 0x00 } else { 0xFF };
    }
}

pub fn fill_orientation(offset: usize, output: &mut [u8]) {
    fill_rotated_orientation(Rotation::Degrees0, offset, output);
}

pub fn fill_rotated_orientation(rotation: Rotation, offset: usize, output: &mut [u8]) {
    let width = rotation.logical_width();
    let height = rotation.logical_height();
    fill_panel_chunk_with(rotation, offset, output, |x, y| {
        orientation_pixel_is_black(x, y, width, height)
    });
}

fn orientation_pixel_is_black(x: usize, y: usize, width: usize, height: usize) -> bool {
    let edge = !(BORDER..width - BORDER).contains(&x) || !(BORDER..height - BORDER).contains(&y);
    let axes = x.abs_diff(width / 2) < 2 || y.abs_diff(height / 2) < 2;
    let right = width - LABEL_MARGIN - LABEL_WIDTH;
    let bottom = height - LABEL_MARGIN - LABEL_HEIGHT;

    edge || axes
        || label_pixel(x, y, LABEL_MARGIN, LABEL_MARGIN, b'T', b'L')
        || label_pixel(x, y, right, LABEL_MARGIN, b'T', b'R')
        || label_pixel(x, y, LABEL_MARGIN, bottom, b'B', b'L')
        || label_pixel(x, y, right, bottom, b'B', b'R')
}

fn label_pixel(
    x: usize,
    y: usize,
    origin_x: usize,
    origin_y: usize,
    first: u8,
    second: u8,
) -> bool {
    let Some(local_x) = x.checked_sub(origin_x) else {
        return false;
    };
    let Some(local_y) = y.checked_sub(origin_y) else {
        return false;
    };
    if local_y >= LABEL_HEIGHT || local_x >= LABEL_WIDTH {
        return false;
    }

    let second_start = GLYPH_WIDTH * GLYPH_SCALE + LABEL_GAP;
    let (glyph, glyph_x) = if local_x < GLYPH_WIDTH * GLYPH_SCALE {
        (first, local_x)
    } else if local_x >= second_start {
        (second, local_x - second_start)
    } else {
        return false;
    };
    let row = local_y / GLYPH_SCALE;
    let column = glyph_x / GLYPH_SCALE;

    glyph_row(glyph, row) & (0b1_0000 >> column) != 0
}

fn glyph_row(glyph: u8, row: usize) -> u8 {
    match glyph {
        b'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ][row],
        b'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ][row],
        b'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ][row],
        b'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ][row],
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::{vec, vec::Vec};

    use super::{fill_checkerboard, fill_orientation, fill_rotated_orientation};
    use crate::display::{
        framebuffer::Rotation,
        ssd1677::{FRAME_BYTES, ROW_BYTES},
    };

    #[test]
    fn checkerboard_uses_forty_pixel_tiles() {
        let mut frame = vec![0; FRAME_BYTES];

        fill_checkerboard(0, &mut frame);

        assert_eq!(frame[0], 0x00);
        assert_eq!(frame[5], 0xFF);
        assert_eq!(frame[40 * ROW_BYTES], 0xFF);
        assert_eq!(frame[40 * ROW_BYTES + 5], 0x00);
    }

    #[test]
    fn orientation_pattern_has_borders_axes_and_four_labels() {
        let mut frame = vec![0; FRAME_BYTES];

        fill_orientation(0, &mut frame);

        assert!(pixel_is_black(&frame, 0, 0));
        assert!(pixel_is_black(&frame, 400, 100));
        assert!(pixel_is_black(&frame, 100, 240));
        assert!(pixel_is_black(&frame, 20, 20));
        assert!(pixel_is_black(&frame, 56, 20));
        assert!(pixel_is_black(&frame, 714, 20));
        assert!(pixel_is_black(&frame, 750, 20));
        assert!(pixel_is_black(&frame, 20, 418));
        assert!(pixel_is_black(&frame, 56, 418));
        assert!(pixel_is_black(&frame, 714, 418));
        assert!(pixel_is_black(&frame, 750, 418));
        assert!(!pixel_is_black(&frame, 100, 100));
    }

    #[test]
    fn degrees270_flips_the_previous_portrait_result_180_degrees() {
        let mut panel = vec![0; FRAME_BYTES];

        fill_rotated_orientation(Rotation::Degrees270, 0, &mut panel);

        assert!(pixel_is_black(&panel, 20, 459));
        assert!(pixel_is_black(&panel, 20, 423));
        assert!(pixel_is_black(&panel, 20, 85));
        assert!(pixel_is_black(&panel, 20, 49));
        assert!(pixel_is_black(&panel, 738, 459));
        assert!(pixel_is_black(&panel, 738, 423));
        assert!(pixel_is_black(&panel, 738, 85));
        assert!(pixel_is_black(&panel, 738, 49));
        assert!(!pixel_is_black(&panel, 100, 379));
    }

    #[test]
    fn pattern_fill_is_independent_of_chunk_boundaries() {
        let mut whole = vec![0; FRAME_BYTES];
        let mut chunked = Vec::with_capacity(FRAME_BYTES);
        fill_orientation(0, &mut whole);

        while chunked.len() < FRAME_BYTES {
            let offset = chunked.len();
            let length = (FRAME_BYTES - offset).min(257);
            let mut chunk = vec![0; length];
            fill_orientation(offset, &mut chunk);
            chunked.extend_from_slice(&chunk);
        }

        assert_eq!(chunked, whole);
    }

    fn pixel_is_black(frame: &[u8], x: usize, y: usize) -> bool {
        frame[y * ROW_BYTES + x / 8] & (0x80 >> (x % 8)) == 0
    }
}
