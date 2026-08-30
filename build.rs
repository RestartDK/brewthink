use std::{
    env,
    error::Error,
    fs, io,
    path::{Path, PathBuf},
};

use brewthink_image::{Dither, MonochromeImage, RenderOptions, RgbImage, ScaleMode, Size};
use image::ImageReader;

const DISPLAY_FRAME_BYTES: usize = 48_000;
const GENERATED_IMAGE_NAME: &str = "brewthink-image.bin";

fn main() {
    linker_be_nice();
    for variable in [
        "BREWTHINK_DISPLAY_STAGE",
        "BREWTHINK_DISPLAY_ROTATION",
        "BREWTHINK_IMAGE_PATH",
        "BREWTHINK_IMAGE_SCALE",
        "BREWTHINK_IMAGE_DITHER",
        "BREWTHINK_IMAGE_EXPORT",
        "BREWTHINK_IMAGE_BUILD_ID",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
    }

    if env::var("TARGET").as_deref() != Ok("riscv32imc-unknown-none-elf") {
        return;
    }

    prepare_image().unwrap_or_else(|error| panic!("failed to prepare display image: {error}"));
    println!("cargo:rustc-link-arg=-Tdefmt.x");
    println!("cargo:rustc-link-arg=-Tlinkall.x");
    println!(
        "cargo:rustc-link-arg=--error-handling-script={}",
        env::current_exe().unwrap().display()
    );
}

fn prepare_image() -> Result<(), Box<dyn Error>> {
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?)
        .join(GENERATED_IMAGE_NAME);

    if env::var("BREWTHINK_DISPLAY_STAGE").as_deref() != Ok("image") {
        fs::write(output, [0xFF; DISPLAY_FRAME_BYTES])?;
        return Ok(());
    }

    let input = PathBuf::from(
        env::var_os("BREWTHINK_IMAGE_PATH")
            .ok_or("BREWTHINK_IMAGE_PATH is required for the image stage")?,
    );
    println!("cargo:rerun-if-changed={}", input.display());

    let rotation = env::var("BREWTHINK_DISPLAY_ROTATION").unwrap_or_else(|_| "270".into());
    let target_size = match rotation.as_str() {
        "0" | "180" => Size::new(800, 480),
        "90" | "270" => Size::new(480, 800),
        _ => return Err(format!("unsupported display rotation {rotation:?}").into()),
    }
    .map_err(image_error)?;
    let scale = match env::var("BREWTHINK_IMAGE_SCALE").as_deref() {
        Ok("cover") => ScaleMode::Cover,
        Ok("contain") | Err(_) => ScaleMode::Contain,
        Ok(value) => return Err(format!("unsupported image scale {value:?}").into()),
    };
    let dither = match env::var("BREWTHINK_IMAGE_DITHER").as_deref() {
        Ok("threshold") => Dither::Threshold(128),
        Ok("ordered") | Err(_) => Dither::Ordered4x4,
        Ok(value) => return Err(format!("unsupported image dither {value:?}").into()),
    };

    let decoded = ImageReader::open(&input)?.with_guessed_format()?.decode()?;
    let rgb = decoded.into_rgb8();
    let source_size =
        Size::new(rgb.width() as usize, rgb.height() as usize).map_err(image_error)?;
    let source = RgbImage::new(source_size, rgb.as_raw()).map_err(image_error)?;
    let mut frame = [0xFF; DISPLAY_FRAME_BYTES];
    let mut target = MonochromeImage::new(target_size, &mut frame).map_err(image_error)?;
    let report = brewthink_image::render(&source, &mut target, RenderOptions { scale, dither });
    fs::write(output, target.as_bytes())?;

    if let Some(export) = env::var_os("BREWTHINK_IMAGE_EXPORT") {
        write_pbm(Path::new(&export), target.size(), target.as_bytes())?;
    }

    println!(
        "cargo:warning=image {}x{} -> {}x{} content {}x{}",
        report.source.width(),
        report.source.height(),
        report.target.width(),
        report.target.height(),
        report.scaled.width(),
        report.scaled.height(),
    );
    Ok(())
}

fn write_pbm(path: &Path, size: Size, pixels: &[u8]) -> io::Result<()> {
    let mut pbm = format!("P4\n{} {}\n", size.width(), size.height()).into_bytes();
    pbm.extend(pixels.iter().map(|byte| !byte));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, pbm)
}

fn image_error(error: brewthink_image::Error) -> io::Error {
    io::Error::other(format!("{error:?}"))
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                what if what.starts_with("_defmt_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`"
                    );
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                what if what.starts_with("esp_rtos_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `esp-radio` has no scheduler enabled. Make sure you have initialized `esp-rtos` or provided an external scheduler."
                    );
                    eprintln!();
                }
                "embedded_test_linker_file_not_added_to_rustflags" => {
                    eprintln!();
                    eprintln!(
                        "💡 `embedded-test` not found - make sure `embedded-test.x` is added as a linker script for tests"
                    );
                    eprintln!();
                }
                "free"
                | "malloc"
                | "calloc"
                | "get_free_internal_heap_size"
                | "malloc_internal"
                | "realloc_internal"
                | "calloc_internal"
                | "free_internal" => {
                    eprintln!();
                    eprintln!(
                        "💡 Did you forget the `esp-alloc` dependency or didn't enable the `compat` feature on it?"
                    );
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }
}
