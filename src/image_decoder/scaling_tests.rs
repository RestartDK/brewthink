extern crate std;

use super::*;

#[test]
fn streamed_pixel_rectangles_match_the_wide_integer_reference() {
    for (width, height) in [
        (1, 1),
        (4096, 1),
        (1, 4096),
        (1200, 1574),
        (320, 240),
        (31, 7),
        (7, 31),
    ] {
        let source = checked_size(width, height).unwrap();
        for (width, height) in [(8, 1), (8, 800), (480, 1), (480, 800), (8192, 1), (8, 8192)] {
            let size = Size::new(width, height).unwrap();
            for mode in [ScaleMode::Contain, ScaleMode::Cover] {
                let scaled = scaled_size(source, size, mode);
                let transform = Transform::new(source, size, mode);
                let mut actual = std::vec![255; width * height / 4];
                let mut expected = actual.clone();
                let mut target =
                    PackedImage::new(size, crate::image::PixelDepth::Four, &mut actual).unwrap();
                let mut reference =
                    PackedImage::new(size, crate::image::PixelDepth::Four, &mut expected).unwrap();
                for y in [0, source.height() / 2, source.height() - 1] {
                    for x in [0, source.width() / 2, source.width() - 1] {
                        target.clear_white();
                        reference.clear_white();
                        transform.draw_source_pixel(&mut target, x, y, 85, Dither::None);
                        let left = (width as i128 - scaled.width() as i128) / 2;
                        let top = (height as i128 - scaled.height() as i128) / 2;
                        let x0 = (left
                            + x as i128 * scaled.width() as i128 / source.width() as i128)
                            .clamp(0, width as i128) as usize;
                        let x1 = (left
                            + (x + 1) as i128 * scaled.width() as i128 / source.width() as i128)
                            .clamp(0, width as i128) as usize;
                        let y0 = (top
                            + y as i128 * scaled.height() as i128 / source.height() as i128)
                            .clamp(0, height as i128) as usize;
                        let y1 = (top
                            + (y + 1) as i128 * scaled.height() as i128 / source.height() as i128)
                            .clamp(0, height as i128) as usize;
                        for destination_y in y0..y1 {
                            for destination_x in x0..x1 {
                                reference.set_luma(destination_x, destination_y, 85);
                            }
                        }
                        assert_eq!(
                            target.bitmap().as_bytes(),
                            reference.bitmap().as_bytes(),
                            "source={source:?} target={size:?} mode={mode:?} pixel=({x},{y})"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn axis_edges_match_wide_division_without_overflow_at_extreme_scales() {
    for source in [1, 2, 3, 7, 127, MAX_IMAGE_DIMENSION] {
        for scaled in [1, 2, 440, 480, 3_276_800, usize::MAX] {
            for target in [1, 480, 800, usize::MAX] {
                let axis = AxisTransform::new(source, scaled, target);
                let offset = (target as i128 - scaled as i128) / 2;
                for position in 0..=source {
                    let expected = (offset + position as i128 * scaled as i128 / source as i128)
                        .clamp(0, target as i128) as usize;
                    assert_eq!(axis.edge(position), expected);
                }
                assert_eq!(axis.range(source), 0..0);
                assert_eq!(axis.range(usize::MAX), 0..0);
            }
        }
    }
}

#[test]
#[ignore = "CPU microbenchmark; not device or end-to-end image latency"]
fn benchmark_streamed_image_scaling() {
    let source = Size::new(1200, 1574).unwrap();
    let size = Size::new(480, 800).unwrap();
    let transform = std::hint::black_box(Transform::new(source, size, ScaleMode::Contain));
    let mut bytes = std::vec![255; 96_000];
    let mut target = PackedImage::new(size, crate::image::PixelDepth::Four, &mut bytes).unwrap();
    for run in 0..5 {
        target.clear_white();
        let start = std::time::Instant::now();
        for y in 0..source.height() {
            for x in 0..source.width() {
                transform.draw_source_pixel(
                    &mut target,
                    std::hint::black_box(x),
                    std::hint::black_box(y),
                    std::hint::black_box(((x * 31 + y * 17) % 256) as u8),
                    Dither::None,
                );
            }
        }
        let elapsed = start.elapsed();
        std::println!(
            "scaling run={run} source=1200x1574 target=480x800 elapsed_us={} crc32={:08x}",
            elapsed.as_micros(),
            crc32fast::hash(std::hint::black_box(target.bitmap().as_bytes()))
        );
    }
}
