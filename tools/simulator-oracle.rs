use std::{convert::Infallible, error::Error, fs, path::Path};

use brewthink::{
    app::{
        App, AppEffect, AppInput, AppPreferences, AppView, Direction, ReaderFont, ReaderFontSize,
        ReaderPreferences, ReaderSpacing, ResumePoint, SleepScreenMode,
    },
    bounded_layout::layout_xhtml_page,
    bounded_xml::FixedString,
    cover::{COVER_HEIGHT, COVER_WIDTH, MAX_ENCODED_COVER_BYTES, encoded_cover_fits},
    device_epub::{
        DeviceEpub, DevicePackageScratch, DevicePublication, MAX_DEVICE_RESOURCE_BYTES,
        MAX_DEVICE_SPINE_ITEMS,
    },
    image::{Dither, PackedBitmap, PackedImage, READER_DEPTH, RenderOptions, ScaleMode, Size},
    image_decoder::{self, ImageFormat, JpegDecodeWorkspace, PngDecodeWorkspace},
    input::UsbState,
    navigation::CHAPTER_TITLE_BYTES,
    power::BatteryStatus,
    reader::{ReaderLine, ReaderView},
    sleep::SleepView,
    storage::MAX_DEVICE_IMAGE_BYTES,
    ui::{AppFrame, render_app},
    zip_stream::{InflateWorkspace, ReadAt, StreamingZip, ZipValidationScratch},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const FRAME_BYTES: usize = 480 * 800 / 8 * READER_DEPTH.bits();

#[derive(Clone, Copy)]
struct File<'a> {
    bytes: &'a [u8],
    length: u32,
}

impl<'a> TryFrom<&'a [u8]> for File<'a> {
    type Error = core::num::TryFromIntError;

    fn try_from(bytes: &'a [u8]) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            length: bytes.len().try_into()?,
            bytes,
        })
    }
}

impl ReadAt for File<'_> {
    type Error = Infallible;
    fn len(&self) -> u32 {
        self.length
    }
    fn read_at(&self, offset: u32, output: &mut [u8]) -> std::result::Result<usize, Self::Error> {
        let offset = (offset as usize).min(self.bytes.len());
        let length = output.len().min(self.bytes.len() - offset);
        output[..length].copy_from_slice(&self.bytes[offset..offset + length]);
        Ok(length)
    }
}

struct Oracle<'publication, 'bytes> {
    book: DeviceEpub<'publication, File<'bytes>>,
    inflater: Box<InflateWorkspace>,
    resource: Box<[u8; MAX_DEVICE_RESOURCE_BYTES]>,
    titles: [FixedString<CHAPTER_TITLE_BYTES>; MAX_DEVICE_SPINE_ITEMS],
    cover: Option<Vec<u8>>,
}

impl Oracle<'_, '_> {
    fn input(&mut self, app: &mut App, input: AppInput) -> Result<()> {
        let effect = app.input(input);
        self.settle(app, effect)
    }

    fn settle(&mut self, app: &mut App, mut effect: AppEffect) -> Result<()> {
        loop {
            effect = match effect {
                AppEffect::LoadChapter { spine_index, .. } => {
                    let length = self
                        .book
                        .read_spine(spine_index, &mut self.resource[..], &mut self.inflater)
                        .map_err(|error| format!("{error:?}"))?;
                    let first =
                        layout_xhtml_page(&self.resource[..length], 0, app.reader_preferences())?;
                    app.chapter_loaded(self.book.publication().spine_len(), first.page_count())
                        .map_err(|error| format!("{error:?}"))?
                }
                AppEffect::Render
                    if matches!(app.view(), AppView::BookCover { .. }) && self.cover.is_none() =>
                {
                    app.input(AppInput::Confirm)
                }
                AppEffect::Render if matches!(app.view(), AppView::Sleeping { .. }) => app
                    .sleep_frame_ready()
                    .map_err(|error| format!("{error:?}"))?,
                AppEffect::None | AppEffect::Render | AppEffect::EnterDeepSleep { .. } => {
                    return Ok(());
                }
            };
        }
    }

    fn start(&mut self, preferences: ReaderPreferences) -> Result<App> {
        let mut app = App::with_catalog(
            4,
            4,
            None,
            AppPreferences::new(preferences, SleepScreenMode::Automatic),
        );
        app.set_battery(BatteryStatus::from_percent(82, UsbState::Disconnected));
        self.input(&mut app, AppInput::Confirm)?;
        self.input(&mut app, AppInput::Confirm)?;
        Ok(app)
    }

    fn begin_reading(&mut self, app: &mut App) -> Result<()> {
        if matches!(app.view(), AppView::BookCover { .. }) {
            self.input(app, AppInput::Confirm)?;
        }
        if !matches!(app.view(), AppView::Reader(_)) {
            return Err("reader state expected".into());
        }
        Ok(())
    }

    fn render(&mut self, app: &App) -> Result<Box<[u8; FRAME_BYTES]>> {
        let mut bytes = Box::new([0xff; FRAME_BYTES]);
        let mut target =
            PackedImage::new(Size::new(480, 800).unwrap(), READER_DEPTH, &mut bytes[..]).unwrap();
        match app.view() {
            AppView::Reader(_) | AppView::ReaderDrawer(_) => {
                let (location, chapter_index) = match app.view() {
                    AppView::Reader(session) => {
                        (session.location(), session.location().spine_index())
                    }
                    AppView::ReaderDrawer(drawer) => {
                        (drawer.session().location(), drawer.chapter())
                    }
                    _ => unreachable!(),
                };
                let length = self
                    .book
                    .read_spine(
                        location.spine_index(),
                        &mut self.resource[..],
                        &mut self.inflater,
                    )
                    .map_err(|error| format!("{error:?}"))?;
                let page = layout_xhtml_page(
                    &self.resource[..length],
                    location.page_index(),
                    app.reader_preferences(),
                )?;
                let lines = page
                    .lines()
                    .map(|line| ReaderLine::new(line.text(), line.style()))
                    .collect::<Vec<_>>();
                let fallback = format!("Chapter {}", chapter_index + 1);
                let title = self
                    .titles
                    .get(chapter_index)
                    .filter(|title| !title.is_empty())
                    .map_or(fallback.as_str(), FixedString::as_str);
                let mut view = ReaderView::new(
                    self.book.publication().title(),
                    title,
                    &lines,
                    app.reader_preferences(),
                    app.battery(),
                );
                if let AppView::ReaderDrawer(drawer) = app.view() {
                    view = view.with_drawer(drawer);
                }
                render_app(AppFrame::Reader(view), &mut target)
                    .map_err(|error| format!("{error:?}"))?;
            }
            AppView::BookCover { .. } => {
                let cover = self.cover.as_ref().ok_or("opening cover is missing")?;
                let bitmap =
                    PackedBitmap::new(Size::new(480, 800).unwrap(), READER_DEPTH, cover).unwrap();
                render_app(AppFrame::Cover(bitmap), &mut target)
                    .map_err(|error| format!("{error:?}"))?;
            }
            AppView::Sleeping { resume, .. } => {
                let ResumePoint::Reader {
                    spine_index,
                    page_index,
                    ..
                } = resume
                else {
                    return Err("oracle sleep requires a reader resume point".into());
                };
                let status = format!(
                    "Chapter {} · page {} · position saved",
                    spine_index + 1,
                    page_index + 1
                );
                let view = match &self.cover {
                    Some(cover) => SleepView::book_cover(
                        PackedBitmap::new(Size::new(480, 800).unwrap(), READER_DEPTH, cover)
                            .unwrap(),
                    ),
                    None => SleepView::built_in(&status, app.battery()),
                };
                render_app(AppFrame::Sleep(view), &mut target)
                    .map_err(|error| format!("{error:?}"))?;
            }
            _ => return Err("oracle render requires cover, reader, drawer or sleep".into()),
        }
        Ok(bytes)
    }

    fn save(&mut self, app: &App, output: &Path, name: &str) -> Result<()> {
        fs::write(
            output.join(format!("{name}.bin")),
            self.render(app)?.as_ref(),
        )?;
        Ok(())
    }
}

fn native_covers(file: File<'_>, path: Option<&str>, output: &Path) -> Result<Option<Vec<u8>>> {
    let mut statuses = String::new();
    let mut full_frame = None;
    let mut scratch = Box::new(ZipValidationScratch::new());
    let archive = StreamingZip::open(file, &mut scratch).map_err(|error| format!("{error:?}"))?;
    for (name, size, maximum, scale) in [
        (
            "shelf",
            Size::new(COVER_WIDTH, COVER_HEIGHT).unwrap(),
            MAX_ENCODED_COVER_BYTES as usize,
            ScaleMode::Cover,
        ),
        (
            "cover",
            Size::new(480, 800).unwrap(),
            MAX_DEVICE_IMAGE_BYTES,
            ScaleMode::Contain,
        ),
    ] {
        let decoded = (|| -> Result<Option<Vec<u8>>> {
            let Some(path) = path else {
                return Ok(None);
            };
            let entry = archive.find(path).map_err(|error| format!("{error:?}"))?;
            if !encoded_cover_fits(entry.compressed_size(), entry.uncompressed_size())
                || entry.uncompressed_size() as usize > maximum
            {
                return Ok(None);
            }
            let mut encoded = vec![0; maximum];
            let length = archive
                .read_entry(entry, &mut encoded, &mut InflateWorkspace::new())
                .map_err(|error| format!("{error:?}"))?;
            let encoded = &encoded[..length];
            let mut bytes = vec![0xff; READER_DEPTH.byte_len(size).unwrap()];
            let mut target = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
            let options = RenderOptions {
                scale,
                dither: Dither::None,
            };
            match ImageFormat::detect(encoded) {
                Some(ImageFormat::Png) => image_decoder::decode_png(
                    encoded,
                    &mut target,
                    options,
                    &mut PngDecodeWorkspace::new(),
                ),
                Some(ImageFormat::Jpeg) => image_decoder::decode_jpeg(
                    encoded,
                    &mut target,
                    options,
                    &mut JpegDecodeWorkspace::new(),
                ),
                None => return Err("unsupported cover format".into()),
            }
            .map_err(|error| format!("{error:?}"))?;
            Ok(Some(bytes))
        })();
        match decoded {
            Ok(Some(bytes)) => {
                statuses.push_str(&format!("{name}: decoded {} bytes\n", bytes.len()));
                fs::write(output.join(format!("{name}.bin")), &bytes)?;
                if name == "cover" {
                    full_frame = Some(bytes);
                }
            }
            Ok(None) => statuses.push_str(&format!("{name}: missing or outside encoded budget\n")),
            Err(error) => statuses.push_str(&format!("{name}: {error}\n")),
        }
    }
    fs::write(output.join("covers.txt"), statuses)?;
    Ok(full_frame)
}

fn drawer_trace(oracle: &mut Oracle<'_, '_>, output: &Path) -> Result<()> {
    use AppInput::{Back, Confirm, Move, Power};
    use Direction::{Down, Left, Right};
    let mut app = oracle.start(ReaderPreferences::default())?;
    oracle.begin_reading(&mut app)?;
    let steps = [
        ("drawer-open", vec![Confirm]),
        ("chapter-row", vec![Move(Down)]),
        ("chapter-next", vec![Move(Right)]),
        ("cancelled", vec![Back]),
        ("jump-open", vec![Confirm]),
        ("jump-row", vec![Move(Down)]),
        ("jump-next", vec![Move(Right)]),
        ("jump-applied", vec![Confirm]),
        ("end-open", vec![Confirm]),
        ("end-position", vec![Move(Right); 20]),
        ("end-applied", vec![Confirm]),
        ("start-open", vec![Confirm]),
        ("start-position", vec![Move(Left); 20]),
        ("start-applied", vec![Confirm]),
        ("type-open", vec![Confirm]),
        ("type-row", vec![Move(Down); 3]),
        ("type-staged", vec![Move(Right)]),
        ("type-applied", vec![Confirm]),
        ("sleep", vec![Power]),
    ];
    let mut trace = String::new();
    for (name, inputs) in steps {
        for input in &inputs {
            oracle.input(&mut app, *input)?;
        }
        oracle.save(&app, output, name)?;
        let keys = inputs
            .iter()
            .map(|input| match input {
                Confirm => "Enter",
                Back => "Escape",
                Power => "p",
                Move(Down) => "ArrowDown",
                Move(Left) => "ArrowLeft",
                Move(Right) => "ArrowRight",
                Move(Direction::Up) => "ArrowUp",
            })
            .collect::<Vec<_>>()
            .join(",");
        let (screen, chapter, page, count) = match app.view() {
            AppView::Reader(session) => (
                "reader",
                session.location().spine_index(),
                session.location().page_index(),
                session.location().page_count(),
            ),
            AppView::ReaderDrawer(drawer) => (
                "reader-drawer",
                drawer.chapter(),
                drawer.session().location().page_index(),
                drawer.session().location().page_count(),
            ),
            AppView::Sleeping { .. } => ("sleep", 0, 0, 0),
            _ => return Err("unexpected trace state".into()),
        };
        trace.push_str(&format!(
            "{name}\t{keys}\t{screen}\t{chapter}\t{page}\t{count}\t{}\n",
            app.reader_preferences().packed()
        ));
    }
    let effect = app.wake();
    oracle.settle(&mut app, effect)?;
    oracle.save(&app, output, "awake")?;
    fs::write(output.join("drawer.txt"), trace)?;
    Ok(())
}

fn main() -> Result<()> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let [input, output] = arguments.as_slice() else {
        return Err("usage: simulator-oracle <epub> <output-directory>".into());
    };
    let output = Path::new(output);
    fs::create_dir_all(output)?;
    let encoded = fs::read(input)?;
    let file = File::try_from(encoded.as_slice())?;
    let mut zip = Box::new(ZipValidationScratch::new());
    let mut package = Box::new(DevicePackageScratch::new());
    let mut inflater = Box::new(InflateWorkspace::new());
    let mut resource = Box::new([0; MAX_DEVICE_RESOURCE_BYTES]);
    let mut publication = Box::new(DevicePublication::new());
    let book = DeviceEpub::open(
        file,
        &mut zip,
        &mut package,
        &mut inflater,
        &mut resource,
        &mut publication,
    )
    .map_err(|error| format!("{error:?}"))?;
    let mut titles = [FixedString::new(); MAX_DEVICE_SPINE_ITEMS];
    let navigation = book.read_chapter_titles(&mut titles, &mut resource[..], &mut inflater);
    fs::write(output.join("navigation.txt"), format!("{navigation:?}\n"))?;
    let cover = native_covers(file, book.publication().cover_path(), output)?;
    let mut oracle = Oracle {
        book,
        inflater,
        resource,
        titles,
        cover,
    };
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
        let mut app = oracle.start(preferences)?;
        oracle.save(&app, output, &format!("opening-{packed}"))?;
        oracle.begin_reading(&mut app)?;
        for spine in 0..oracle.book.publication().spine_len() {
            let AppView::Reader(session) = app.view() else {
                return Err("reader state expected".into());
            };
            let count = session.location().page_count();
            cases.push_str(&format!("{packed} {spine} {count}\n"));
            for page in 0..count {
                let AppView::Reader(session) = app.view() else {
                    return Err("reader state expected".into());
                };
                if session.location().spine_index() != spine
                    || session.location().page_index() != page
                {
                    return Err("chapter transition disagrees with bounded layout".into());
                }
                oracle.save(&app, output, &format!("{packed}-{spine}-{page}"))?;
                oracle.input(&mut app, AppInput::Move(Direction::Right))?;
            }
        }
    }
    fs::write(output.join("pages.txt"), cases)?;
    drawer_trace(&mut oracle, output)?;
    Ok(())
}
