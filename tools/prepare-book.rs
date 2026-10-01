use std::{
    env,
    error::Error,
    ffi::OsString,
    fs,
    io::{Cursor, Read, Write},
    path::PathBuf,
};

use image::{
    ExtendedColorType, ImageEncoder, ImageReader,
    codecs::{jpeg::JpegEncoder, png::PngEncoder},
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const JPEG_QUALITY: u8 = 88;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args_os().skip(1);
    let input = PathBuf::from(required(&mut arguments, "INPUT")?);
    let output = PathBuf::from(required(&mut arguments, "OUTPUT")?);
    if let Some(argument) = arguments.next() {
        return Err(format!("unexpected argument {argument:?}").into());
    }
    let encoded = fs::read(&input)?;
    let (converted, count) = rewrite_images(&encoded)?;
    fs::write(&output, &converted)?;
    println!(
        "prepared {} -> {} ({} images re-encoded, {} bytes)",
        input.display(),
        output.display(),
        count,
        converted.len()
    );
    Ok(())
}

fn required(
    arguments: &mut impl Iterator<Item = OsString>,
    name: &str,
) -> Result<OsString, Box<dyn Error>> {
    arguments
        .next()
        .ok_or_else(|| format!("missing {name}").into())
}

fn rewrite_images(encoded: &[u8]) -> Result<(Vec<u8>, usize), Box<dyn Error>> {
    let mut archive = ZipArchive::new(Cursor::new(encoded))?;
    let mut output = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(&mut output);
    let mut converted = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        let (data, changed) = convert_image(&data)?;
        converted += usize::from(changed);
        let method = if name == "mimetype" {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        writer.start_file(
            name,
            SimpleFileOptions::default().compression_method(method),
        )?;
        writer.write_all(&data)?;
    }
    writer.finish()?;
    Ok((output.into_inner(), converted))
}

fn convert_image(data: &[u8]) -> Result<(Vec<u8>, bool), Box<dyn Error>> {
    if jpeg_needs_reencode(data) {
        let decoded = ImageReader::new(Cursor::new(data))
            .with_guessed_format()?
            .decode()?;
        let rgb = decoded.into_rgb8();
        let mut encoded = Vec::new();
        JpegEncoder::new_with_quality(&mut encoded, JPEG_QUALITY).encode(
            &rgb,
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        )?;
        return Ok((encoded, true));
    }
    if png_needs_reencode(data) {
        let decoded = ImageReader::new(Cursor::new(data))
            .with_guessed_format()?
            .decode()?;
        let rgba = decoded.into_rgba8();
        let mut encoded = Vec::new();
        PngEncoder::new(&mut encoded).write_image(
            &rgba,
            rgba.width(),
            rgba.height(),
            ExtendedColorType::Rgba8,
        )?;
        return Ok((encoded, true));
    }
    Ok((data.to_vec(), false))
}

fn jpeg_needs_reencode(data: &[u8]) -> bool {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return false;
    }
    let mut index = 2;
    while index + 3 < data.len() {
        if data[index] != 0xFF {
            index += 1;
            continue;
        }
        let marker = data[index + 1];
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            index += 2;
            continue;
        }
        if marker == 0xDA {
            break;
        }
        let length = usize::from(u16::from_be_bytes([data[index + 2], data[index + 3]]));
        if length < 2 || index + 2 + length > data.len() {
            break;
        }
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            return marker != 0xC0;
        }
        index += 2 + length;
    }
    false
}

fn png_needs_reencode(data: &[u8]) -> bool {
    data.len() >= 29
        && &data[..8] == b"\x89PNG\r\n\x1a\n"
        && &data[12..16] == b"IHDR"
        && data[28] != 0
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use image::{
        ExtendedColorType, RgbImage,
        codecs::jpeg::{JpegEncoder, PixelDensity},
    };

    use super::{jpeg_needs_reencode, png_needs_reencode, rewrite_images};

    fn baseline_jpeg() -> Vec<u8> {
        let image = RgbImage::from_fn(8, 8, |x, y| image::Rgb([x as u8 * 31, y as u8 * 31, 128]));
        let mut encoded = Vec::new();
        let mut encoder = JpegEncoder::new_with_quality(&mut encoded, 90);
        encoder.set_pixel_density(PixelDensity::default());
        encoder
            .encode(
                image.as_raw(),
                image.width(),
                image.height(),
                ExtendedColorType::Rgb8,
            )
            .unwrap();
        encoded
    }

    #[test]
    fn jpeg_detection_reads_the_frame_marker() {
        let mut progressive = vec![0xFF, 0xD8, 0xFF, 0xC2, 0x00, 0x0B, 0x08];
        progressive.extend_from_slice(&[0; 8]);
        assert!(jpeg_needs_reencode(&progressive));

        // A segment whose last payload byte is 0xFF must not derail the walk.
        let mut trailing_payload = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x03, 0xFF];
        trailing_payload.extend_from_slice(&[0xFF, 0xC2, 0x00, 0x0B, 0x08]);
        trailing_payload.extend_from_slice(&[0; 8]);
        assert!(jpeg_needs_reencode(&trailing_payload));

        let baseline = baseline_jpeg();
        assert!(!jpeg_needs_reencode(&baseline));

        assert!(!jpeg_needs_reencode(b"not a jpeg"));
    }

    #[test]
    fn png_detection_reads_the_interlace_byte() {
        let mut header = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        header.extend_from_slice(&[0, 0, 0, 13]);
        header.extend_from_slice(b"IHDR");
        header.extend_from_slice(&[0; 12]);
        header.push(0);
        assert!(!png_needs_reencode(&header));
        header[28] = 1;
        assert!(png_needs_reencode(&header));
    }

    #[test]
    fn rewriting_keeps_the_mimetype_first_and_other_entries_byte_equal() {
        use zip::{ZipWriter, write::SimpleFileOptions};

        let jpeg = baseline_jpeg();
        let mut encoded = std::io::Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut encoded);
            let stored =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for (name, bytes) in [
                ("mimetype", b"application/epub+zip".as_slice()),
                ("META-INF/container.xml", b"<container/>".as_slice()),
                ("OPS/cover.jpg", jpeg.as_slice()),
            ] {
                writer.start_file(name, stored).unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer.finish().unwrap();
        }

        let (converted, count) = rewrite_images(&encoded.into_inner()).unwrap();
        assert_eq!(count, 0);

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(converted)).unwrap();
        assert_eq!(archive.by_index(0).unwrap().name(), "mimetype");
        assert_eq!(
            archive.by_index(0).unwrap().compression(),
            zip::CompressionMethod::Stored
        );
        let mut cover = Vec::new();
        archive
            .by_index(2)
            .unwrap()
            .read_to_end(&mut cover)
            .unwrap();
        assert_eq!(cover, jpeg);
    }
}
