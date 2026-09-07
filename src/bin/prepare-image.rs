use std::{
    env,
    error::Error,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

use brewthink::image::{Dither, PackedImage, PixelDepth, RenderOptions, RgbImage, ScaleMode, Size};
use image::ImageReader;

struct Arguments {
    input: PathBuf,
    frame: PathBuf,
    preview: PathBuf,
    size: Size,
    depth: PixelDepth,
    options: RenderOptions,
}

impl Arguments {
    fn parse() -> Result<Self, Box<dyn Error>> {
        let mut arguments = env::args_os().skip(1);
        let input = PathBuf::from(required(&mut arguments, "INPUT")?);
        let frame = PathBuf::from(required(&mut arguments, "FRAME")?);
        let preview = PathBuf::from(required(&mut arguments, "PREVIEW")?);
        let width = parse_usize(required(&mut arguments, "WIDTH")?, "WIDTH")?;
        let height = parse_usize(required(&mut arguments, "HEIGHT")?, "HEIGHT")?;
        let scale = match required_utf8(&mut arguments, "SCALE")?.as_str() {
            "contain" => ScaleMode::Contain,
            "cover" => ScaleMode::Cover,
            value => return Err(format!("unsupported scale {value:?}").into()),
        };
        let (depth, dither) = match required_utf8(&mut arguments, "QUANTIZER")?.as_str() {
            "ordered" => (PixelDepth::Monochrome, Dither::Ordered4x4),
            "threshold" => (PixelDepth::Monochrome, Dither::Threshold(128)),
            "gray4" => (PixelDepth::Four, Dither::None),
            "gray8" => (PixelDepth::Eight, Dither::None),
            value => {
                return Err(format!(
                    "unsupported quantizer {value:?}; use ordered, threshold, gray4, or gray8"
                )
                .into());
            }
        };
        if let Some(argument) = arguments.next() {
            return Err(format!("unexpected argument {argument:?}").into());
        }

        Ok(Self {
            input,
            frame,
            preview,
            size: Size::new(width, height).map_err(image_error)?,
            depth,
            options: RenderOptions { scale, dither },
        })
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = Arguments::parse()?;
    let decoded = ImageReader::open(&arguments.input)?
        .with_guessed_format()?
        .decode()?;
    let rgba = decoded.into_rgba8();
    let source_size =
        Size::new(rgba.width() as usize, rgba.height() as usize).map_err(image_error)?;
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.pixels() {
        let alpha = u16::from(pixel[3]);
        for &channel in &pixel.0[..3] {
            rgb.push(((u16::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8);
        }
    }
    let source = RgbImage::new(source_size, &rgb).map_err(image_error)?;
    let mut frame = vec![
        0xFF;
        arguments
            .depth
            .byte_len(arguments.size)
            .map_err(image_error)?
    ];
    let mut target =
        PackedImage::new(arguments.size, arguments.depth, &mut frame).map_err(image_error)?;
    let report = brewthink::image::render(&source, &mut target, arguments.options);

    write_file(&arguments.frame, target.as_bytes())?;
    if arguments.depth == PixelDepth::Monochrome {
        write_pbm(&arguments.preview, target.size(), target.as_bytes())?;
    } else {
        let mut preview = format!(
            "P5\n{} {}\n255\n",
            target.size().width(),
            target.size().height()
        )
        .into_bytes();
        for y in 0..target.size().height() {
            for x in 0..target.size().width() {
                preview.push(target.luma(x, y));
            }
        }
        write_file(&arguments.preview, &preview)?;
    }

    println!(
        "image {}x{} -> {}x{} content {}x{}",
        report.source.width(),
        report.source.height(),
        report.target.width(),
        report.target.height(),
        report.scaled.width(),
        report.scaled.height(),
    );
    println!("packed frame: {}", arguments.frame.display());
    println!(
        "{}-tone preview: {}",
        arguments.depth.levels(),
        arguments.preview.display()
    );
    Ok(())
}

fn required(
    arguments: &mut impl Iterator<Item = OsString>,
    name: &str,
) -> Result<OsString, Box<dyn Error>> {
    arguments.next().ok_or_else(|| {
        format!("missing {name}; expected INPUT FRAME PREVIEW WIDTH HEIGHT SCALE DITHER").into()
    })
}

fn required_utf8(
    arguments: &mut impl Iterator<Item = OsString>,
    name: &str,
) -> Result<String, Box<dyn Error>> {
    required(arguments, name)?
        .into_string()
        .map_err(|value| format!("{name} is not valid UTF-8: {value:?}").into())
}

fn parse_usize(value: OsString, name: &str) -> Result<usize, Box<dyn Error>> {
    let value = value
        .into_string()
        .map_err(|value| format!("{name} is not valid UTF-8: {value:?}"))?;
    value
        .parse()
        .map_err(|error| format!("invalid {name} {value:?}: {error}").into())
}

fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)
}

fn write_pbm(path: &Path, size: Size, pixels: &[u8]) -> io::Result<()> {
    let mut pbm = format!("P4\n{} {}\n", size.width(), size.height()).into_bytes();
    pbm.extend(pixels.iter().map(|byte| !byte));
    write_file(path, &pbm)
}

fn image_error(error: brewthink::image::Error) -> io::Error {
    io::Error::other(format!("{error:?}"))
}
