use brewthink::{
    image::{Dither, PackedBitmap, PackedImage, READER_DEPTH, RenderOptions, ScaleMode},
    image_cache::{ImageSpec, ImageWorkspace},
    image_decoder::{
        ImageFormat, JpegDecodeWorkspace, PngDecodeWorkspace, decode_jpeg, decode_png,
    },
    zip_stream::{ReadAt, StreamingZip, ZipValidationScratch},
};
use std::{
    fs::File,
    io::{self, Read, Seek, Write},
    os::unix::fs::FileExt,
    path::{Path, PathBuf},
    time::Instant,
};

struct LocalFile {
    file: File,
    length: u32,
}
impl LocalFile {
    fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let length = u32::try_from(file.metadata()?.len()).map_err(io::Error::other)?;
        Ok(Self { file, length })
    }
}
impl ReadAt for LocalFile {
    type Error = io::Error;
    fn len(&self) -> u32 {
        self.length
    }
    fn read_at(&self, offset: u32, output: &mut [u8]) -> io::Result<usize> {
        self.file.read_at(output, u64::from(offset))
    }
}

struct Stage {
    file: File,
    path: PathBuf,
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .ok_or("usage: inspect-images EPUB OUTPUT_DIRECTORY")?,
    );
    let output = PathBuf::from(args.next().ok_or("missing output directory")?);
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    std::fs::create_dir_all(&output)?;
    let stage_path = output.join(format!("source-{}.tmp", std::process::id()));
    let mut stage = Stage {
        file: File::create_new(&stage_path)?,
        path: stage_path,
    };
    let mut index = zip::ZipArchive::new(File::open(&source)?)?;
    let names: Vec<String> = index
        .file_names()
        .map(String::from)
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg")
        })
        .collect();
    let mut zip = Box::new(ZipValidationScratch::new());
    let archive = StreamingZip::open(LocalFile::open(&source)?, &mut zip)
        .map_err(|error| format!("ZIP: {error:?}"))?;
    let mut workspace = Box::new(ImageWorkspace::new());
    let spec = ImageSpec::new(480, 800, ScaleMode::Contain).unwrap();
    let mut pixels = vec![0; spec.byte_len()];
    let mut reference = vec![0; spec.byte_len()];
    let options = RenderOptions {
        scale: ScaleMode::Contain,
        dither: Dither::None,
    };
    let mut failures = 0;
    let mut captures = 0;
    for (number, name) in names.iter().enumerate() {
        let started = Instant::now();
        let result = (|| -> Result<(), String> {
            let entry = archive
                .find(name)
                .map_err(|error| format!("entry: {error:?}"))?;
            let dimensions = workspace
                .probe(&archive, entry)
                .map_err(|error| format!("probe: {error:?}"))?;
            stage.file.set_len(0).map_err(|error| error.to_string())?;
            stage.file.rewind().map_err(|error| error.to_string())?;
            archive
                .read_entry_to(entry, workspace.inflate(), &mut [0; 4096], |bytes| {
                    stage.file.write_all(bytes)
                })
                .map_err(|error| format!("extract: {error:?}"))?;
            let staged = LocalFile::open(&stage.path).map_err(|error| error.to_string())?;
            let decoded = workspace
                .decode(&staged, spec, &mut pixels)
                .map_err(|error| format!("decode: {error:?}"))?;
            if decoded != dimensions {
                return Err("probe/decode dimension disagreement".into());
            }
            let encoded = std::fs::read(&stage.path).map_err(|error| error.to_string())?;
            let mut oracle = Vec::new();
            index
                .by_name(name)
                .map_err(|error| error.to_string())?
                .read_to_end(&mut oracle)
                .map_err(|error| error.to_string())?;
            if encoded != oracle {
                return Err("ZIP oracle disagreement".into());
            }
            let mut target = PackedImage::new(spec.size(), READER_DEPTH, &mut reference).unwrap();
            match ImageFormat::detect(&encoded).ok_or("missing image signature")? {
                ImageFormat::Png => decode_png(
                    &encoded,
                    &mut target,
                    options,
                    &mut Box::new(PngDecodeWorkspace::new()),
                ),
                ImageFormat::Jpeg => decode_jpeg(
                    &encoded,
                    &mut target,
                    options,
                    &mut Box::new(JpegDecodeWorkspace::new()),
                ),
            }
            .map_err(|error| format!("slice oracle: {error:?}"))?;
            if pixels != reference {
                return Err("stream/slice pixel disagreement".into());
            }
            if captures == 0 || name.to_ascii_lowercase().contains("cover") {
                let bitmap = PackedBitmap::new(spec.size(), READER_DEPTH, &pixels).unwrap();
                let mut png = image::GrayImage::new(480, 800);
                for (x, y, pixel) in png.enumerate_pixels_mut() {
                    *pixel = image::Luma([bitmap.luma(x as usize, y as usize)]);
                }
                png.save(output.join(format!("image-{number:04}.png")))
                    .map_err(|error| error.to_string())?;
                captures += 1;
            }
            println!(
                "OK image={number} path={name:?} bytes={} width={} height={} crc32={:08x} host_ms={}",
                entry.uncompressed_size(),
                decoded.width(),
                decoded.height(),
                crc32fast::hash(&pixels),
                started.elapsed().as_millis()
            );
            Ok(())
        })();
        if let Err(error) = result {
            failures += 1;
            println!("FAIL image={number} path={name:?} reason={error}");
        }
    }
    println!(
        "SUMMARY images={} failed={failures} captures={captures} workspace_bytes={}",
        names.len(),
        core::mem::size_of::<ImageWorkspace>()
    );
    if failures != 0 {
        return Err(format!("{failures} image checks failed").into());
    }
    Ok(())
}
