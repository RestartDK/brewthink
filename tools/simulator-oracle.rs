use std::{convert::Infallible, error::Error, fs, path::Path};

use brewthink::{
    app::{
        App, AppInput, AppPreferences, AppView, Direction, ReaderFont, ReaderFontSize,
        ReaderPreferences, ReaderSpacing, SleepScreenMode,
    },
    bounded_layout::layout_xhtml_page,
    cover::{self, COVER_BYTES, CoverDecodeWorkspace, JpegDecodeWorkspace},
    device_epub::{DeviceEpub, DevicePackageScratch, DevicePublication, MAX_DEVICE_RESOURCE_BYTES},
    image::{PackedImage, READER_DEPTH, Size},
    image_decoder::ImageFormat,
    input::UsbState,
    power::BatteryStatus,
    reader::{ReaderLine, ReaderView},
    sleep::SleepView,
    ui::{AppFrame, render_app},
    zip_stream::{InflateWorkspace, ReadAt, ZipValidationScratch},
};

const FRAME_BYTES: usize = 480 * 800 / 8 * READER_DEPTH.bits();

struct File {
    bytes: Vec<u8>,
    length: u32,
}

impl TryFrom<Vec<u8>> for File {
    type Error = core::num::TryFromIntError;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        Ok(Self {
            length: bytes.len().try_into()?,
            bytes,
        })
    }
}

impl ReadAt for File {
    type Error = Infallible;
    fn len(&self) -> u32 {
        self.length
    }
    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        let offset = (offset as usize).min(self.bytes.len());
        let length = output.len().min(self.bytes.len() - offset);
        output[..length].copy_from_slice(&self.bytes[offset..offset + length]);
        Ok(length)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let [input, output] = arguments.as_slice() else {
        return Err("usage: simulator-oracle <epub> <output-directory>".into());
    };
    let output = Path::new(output);
    fs::create_dir_all(output)?;
    let mut zip = Box::new(ZipValidationScratch::new());
    let mut package = Box::new(DevicePackageScratch::new());
    let mut inflater = Box::new(InflateWorkspace::new());
    let mut resource = Box::new([0; MAX_DEVICE_RESOURCE_BYTES]);
    let mut publication = Box::new(DevicePublication::new());
    let book = DeviceEpub::open(
        File::try_from(fs::read(input)?)?,
        &mut zip,
        &mut package,
        &mut inflater,
        &mut resource,
        &mut publication,
    )
    .map_err(|error| format!("{error:?}"))?;
    let mut cases = String::new();
    for preferences in [
        ReaderPreferences::default(),
        ReaderPreferences::new(
            ReaderFont::Mono,
            ReaderFontSize::Large,
            ReaderSpacing::Relaxed,
        ),
    ] {
        let packed = preferences.packed();
        let mut app = App::with_catalog(
            4,
            4,
            None,
            AppPreferences::new(preferences, SleepScreenMode::Automatic),
        );
        app.set_battery(BatteryStatus::from_percent(82, UsbState::Disconnected));
        app.input(AppInput::Confirm);
        app.input(AppInput::Confirm);
        for spine in 0..book.publication().spine_len() {
            let length = book
                .read_spine(spine, &mut resource[..], &mut inflater)
                .map_err(|error| format!("{error:?}"))?;
            let first = layout_xhtml_page(&resource[..length], 0, preferences)?;
            app.chapter_loaded(book.publication().spine_len(), first.page_count())
                .map_err(|error| format!("{error:?}"))?;
            cases.push_str(&format!("{packed} {spine} {}\n", first.page_count()));
            for index in 0..first.page_count() {
                let page = layout_xhtml_page(&resource[..length], index, preferences)?;
                let AppView::Reader(session) = app.view() else {
                    return Err("reader state expected".into());
                };
                let lines = page
                    .lines()
                    .map(|line| ReaderLine::new(line.text(), line.style()))
                    .collect::<Vec<_>>();
                let mut bytes = [0xff; FRAME_BYTES];
                let mut frame =
                    PackedImage::new(Size::new(480, 800).unwrap(), READER_DEPTH, &mut bytes)
                        .unwrap();
                render_app(
                    AppFrame::Reader(ReaderView::new(
                        book.publication().title(),
                        page.chapter_title(),
                        &lines,
                        session.location(),
                        preferences,
                        app.battery(),
                    )),
                    &mut frame,
                )
                .map_err(|error| format!("{error:?}"))?;
                fs::write(output.join(format!("{packed}-{spine}-{index}.bin")), bytes)?;
                app.input(AppInput::Move(Direction::Right));
            }
        }
    }
    fs::write(output.join("pages.txt"), cases)?;
    if let Some(length) = book
        .read_cover(&mut resource[..], &mut inflater)
        .map_err(|error| format!("{error:?}"))?
    {
        let encoded = &resource[..length];
        let mut cover_bytes = [0xff; COVER_BYTES];
        match ImageFormat::detect(encoded) {
            Some(ImageFormat::Png) => {
                cover::decode_png_cover(encoded, &mut cover_bytes, &mut CoverDecodeWorkspace::new())
            }
            Some(ImageFormat::Jpeg) => {
                cover::decode_jpeg_cover(encoded, &mut cover_bytes, &mut JpegDecodeWorkspace::new())
            }
            None => return Err("oracle cover must be PNG or JPEG".into()),
        }
        .map_err(|error| format!("{error:?}"))?;
        let mut bytes = [0xff; FRAME_BYTES];
        let mut frame =
            PackedImage::new(Size::new(480, 800).unwrap(), READER_DEPTH, &mut bytes).unwrap();
        render_app(
            AppFrame::Sleep(SleepView::book_cover(
                book.publication().title(),
                book.publication().creator(),
                "CHAPTER 1 · PAGE 1 · POSITION SAVED",
                cover::bitmap(&cover_bytes),
                BatteryStatus::from_percent(82, UsbState::Disconnected),
            )),
            &mut frame,
        )
        .map_err(|error| format!("{error:?}"))?;
        fs::write(output.join("sleep.bin"), bytes)?;
    }
    Ok(())
}
