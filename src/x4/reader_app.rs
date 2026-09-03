use core::fmt::Write;

use defmt::info;
use embassy_futures::select::{Either, select};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use embassy_time::{Duration, Timer};
use embedded_sdmmc::{TimeSource, Timestamp};
use esp_hal::{
    Blocking,
    delay::Delay,
    gpio::{Input, InputConfig, Pull, RtcPinWithResistors},
    peripherals::{GPIO3, LPWR},
    rtc_cntl::{
        Rtc,
        sleep::{RtcioWakeupSource, WakeupLevel},
    },
    system::SleepSource,
    usb_serial_jtag::UsbSerialJtagRx,
};
use static_cell::StaticCell;

use crate::{
    app::{
        App, AppEffect, AppInput, AppView, BookId, BookOrigin, Direction, HomeItem,
        ReaderPreferences, ReadingLocation, ResumePoint, SettingsItem,
    },
    bounded_layout::{BoundedPage, MAX_PAGE_LINES, layout_xhtml_page_into},
    bounded_xml::FixedString,
    cover::{
        COVER_BYTES, CoverDecodeWorkspace, JpegDecodeWorkspace, bitmap, decode_jpeg_cover,
        decode_png_cover, encoded_cover_fits,
    },
    device_epub::{
        DeviceEpub, DevicePackageScratch, MAX_DEVICE_PATH_BYTES, MAX_DEVICE_RESOURCE_BYTES,
    },
    display::{
        framebuffer::{FRAME_BYTES, Rotation},
        ssd1677::{BufferedDisplay, RefreshPolicy, RefreshPolicyMode, Ssd1677, X4DriveProfile},
    },
    files::{FileItem, render_files},
    home::render_home,
    image::{MonochromeBitmap, MonochromeImage, Size},
    input::{
        Button, ButtonDebouncer, ButtonEvent, ButtonTransition, PressedButtons,
        control::{ControlCommand, ControlLineBuffer},
    },
    library::{ShelfBook, render_shelf, render_shelf_cover},
    power::{BatteryEstimator, BatteryStatus},
    reader::{ReaderLine, ReaderStyle, ReaderView, render_reader, render_reader_error},
    settings::render_settings,
    sleep::{SleepView, render_sleep},
    storage::{BookFile, ReadOnlyFatBookStore, ReadOnlySdCard},
    x4::{X4InputHardware, X4ReadOnlyFatBlockDevice, X4StorageHardware, decode_buttons},
    zip_stream::{InflateWorkspace, StreamingZip, ZipValidationScratch},
};

const MAX_DEVICE_BOOKS: usize = 16;
const MAX_CACHED_SPINE_PATHS: usize = 64;
const MAX_CACHED_SPINE_PATH_BYTES: usize = 2 * 1024;
const VISIBLE_COVER_SLOTS: usize = 4;
const SHELF_COVER_WIDTH: usize = 88;
const SHELF_COVER_HEIGHT: usize = 132;
const SHELF_COVER_BYTES: usize = SHELF_COVER_WIDTH * SHELF_COVER_HEIGHT / 8;
const X4_DRIVE_PROFILE: &str = match option_env!("BREWTHINK_X4_DRIVE_PROFILE") {
    Some(profile) => profile,
    None => "stock-parity",
};
const DISPLAY_REFRESH: &str = match option_env!("BREWTHINK_DISPLAY_REFRESH") {
    Some(mode) => mode,
    None => "automatic",
};
type DeviceStore = ReadOnlyFatBookStore<X4ReadOnlyFatBlockDevice<'static>, FixedTimeSource>;

struct ReaderDisplay {
    display: BufferedDisplay<'static>,
    refresh_policy: RefreshPolicy,
}

#[derive(Clone, Copy)]
struct FixedTimeSource;

impl TimeSource for FixedTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

struct DeviceLibrary {
    files: [Option<BookFile>; MAX_DEVICE_BOOKS],
    titles: [FixedString<192>; MAX_DEVICE_BOOKS],
    creators: [FixedString<128>; MAX_DEVICE_BOOKS],
    cover_paths: [Option<FixedString<MAX_DEVICE_PATH_BYTES>>; MAX_DEVICE_BOOKS],
    book_spine_starts: [u8; MAX_DEVICE_BOOKS],
    book_cached_spine_counts: [u8; MAX_DEVICE_BOOKS],
    spine_counts: [u8; MAX_DEVICE_BOOKS],
    spine_path_offsets: [u16; MAX_CACHED_SPINE_PATHS],
    spine_path_lengths: [u8; MAX_CACHED_SPINE_PATHS],
    spine_path_bytes: [u8; MAX_CACHED_SPINE_PATH_BYTES],
    spine_path_count: u8,
    spine_path_byte_length: u16,
    length: usize,
}

impl DeviceLibrary {
    const fn empty() -> Self {
        Self {
            files: [None; MAX_DEVICE_BOOKS],
            titles: [FixedString::new(); MAX_DEVICE_BOOKS],
            creators: [FixedString::new(); MAX_DEVICE_BOOKS],
            cover_paths: [None; MAX_DEVICE_BOOKS],
            book_spine_starts: [0; MAX_DEVICE_BOOKS],
            book_cached_spine_counts: [0; MAX_DEVICE_BOOKS],
            spine_counts: [0; MAX_DEVICE_BOOKS],
            spine_path_offsets: [0; MAX_CACHED_SPINE_PATHS],
            spine_path_lengths: [0; MAX_CACHED_SPINE_PATHS],
            spine_path_bytes: [0; MAX_CACHED_SPINE_PATH_BYTES],
            spine_path_count: 0,
            spine_path_byte_length: 0,
            length: 0,
        }
    }

    fn file(&self, book: BookId) -> Option<BookFile> {
        self.files.get(book.index()).copied().flatten()
    }

    fn title(&self, book: BookId) -> &str {
        self.titles
            .get(book.index())
            .map_or("Unknown title", FixedString::as_str)
    }

    fn creator(&self, book: BookId) -> &str {
        self.creators
            .get(book.index())
            .map_or("Unknown creator", FixedString::as_str)
    }

    fn cover_path(&self, book: BookId) -> Option<&str> {
        self.cover_paths
            .get(book.index())
            .and_then(Option::as_ref)
            .map(FixedString::as_str)
    }

    fn spine_count(&self, book: BookId) -> usize {
        self.spine_counts
            .get(book.index())
            .copied()
            .map_or(0, usize::from)
    }

    fn cache_spine_paths(
        &mut self,
        book: BookId,
        publication: &crate::device_epub::DevicePublication,
    ) {
        let start = usize::from(self.spine_path_count);
        let mut cached = 0usize;
        for spine_index in 0..publication.spine_len() {
            let Some(item) = publication.spine_item(spine_index) else {
                break;
            };
            let path = item.path().as_bytes();
            let path_index = start + cached;
            let byte_start = usize::from(self.spine_path_byte_length);
            let Some(byte_end) = byte_start.checked_add(path.len()) else {
                break;
            };
            if path_index >= self.spine_path_offsets.len() || byte_end > self.spine_path_bytes.len()
            {
                break;
            }
            let Ok(offset) = u16::try_from(byte_start) else {
                break;
            };
            let Ok(length) = u8::try_from(path.len()) else {
                break;
            };
            self.spine_path_bytes[byte_start..byte_end].copy_from_slice(path);
            self.spine_path_offsets[path_index] = offset;
            self.spine_path_lengths[path_index] = length;
            self.spine_path_count = self.spine_path_count.saturating_add(1);
            self.spine_path_byte_length = u16::try_from(byte_end).unwrap_or(u16::MAX);
            cached += 1;
        }
        self.book_spine_starts[book.index()] = u8::try_from(start).unwrap_or(u8::MAX);
        self.book_cached_spine_counts[book.index()] = u8::try_from(cached).unwrap_or(u8::MAX);
    }

    fn spine_path(&self, book: BookId, spine_index: usize) -> Option<&str> {
        let cached = usize::from(*self.book_cached_spine_counts.get(book.index())?);
        if spine_index >= cached {
            return None;
        }
        let start = usize::from(*self.book_spine_starts.get(book.index())?);
        let path_index = start.checked_add(spine_index)?;
        let offset = usize::from(*self.spine_path_offsets.get(path_index)?);
        let length = usize::from(*self.spine_path_lengths.get(path_index)?);
        let end = offset.checked_add(length)?;
        core::str::from_utf8(self.spine_path_bytes.get(offset..end)?).ok()
    }

    fn file_name(&self, book: BookId) -> &str {
        self.files
            .get(book.index())
            .and_then(Option::as_ref)
            .map_or("Unknown.epub", |file| file.name().as_str())
    }

    fn file_size(&self, book: BookId) -> u32 {
        self.files
            .get(book.index())
            .and_then(Option::as_ref)
            .map_or(0, BookFile::size)
    }
}

#[repr(C, align(8))]
struct FrameCodecWorkspace {
    bytes: [u8; FRAME_BYTES],
}

impl FrameCodecWorkspace {
    const fn new() -> Self {
        Self {
            bytes: [0; FRAME_BYTES],
        }
    }

    fn frame(&mut self) -> &mut [u8; FRAME_BYTES] {
        &mut self.bytes
    }

    fn prepare_inflate(&mut self) -> &mut InflateWorkspace {
        const {
            assert!(core::mem::size_of::<InflateWorkspace>() <= FRAME_BYTES);
            assert!(
                core::mem::align_of::<InflateWorkspace>()
                    <= core::mem::align_of::<FrameCodecWorkspace>()
            );
            assert!(!core::mem::needs_drop::<InflateWorkspace>());
        }
        let workspace = self.bytes.as_mut_ptr().cast::<InflateWorkspace>();
        // SAFETY: the storage is aligned, large enough, and exclusively borrowed. The
        // initializer establishes a valid InflateWorkspace before the reference is created.
        unsafe {
            InflateWorkspace::initialize_in_place(workspace);
            &mut *workspace
        }
    }

    fn with_png<R>(&mut self, function: impl FnOnce(&mut CoverDecodeWorkspace) -> R) -> R {
        const {
            assert!(core::mem::size_of::<CoverDecodeWorkspace>() <= FRAME_BYTES);
            assert!(core::mem::align_of::<CoverDecodeWorkspace>() == 1);
        }
        // SAFETY: the workspace contains only byte arrays, fits, and has byte alignment.
        let workspace = unsafe { &mut *self.bytes.as_mut_ptr().cast::<CoverDecodeWorkspace>() };
        function(workspace)
    }

    fn with_jpeg<R>(&mut self, function: impl FnOnce(&mut JpegDecodeWorkspace) -> R) -> R {
        const {
            assert!(core::mem::size_of::<JpegDecodeWorkspace>() <= FRAME_BYTES);
            assert!(core::mem::align_of::<JpegDecodeWorkspace>() == 1);
        }
        // SAFETY: the workspace contains one byte array, fits, and has byte alignment.
        let workspace = unsafe { &mut *self.bytes.as_mut_ptr().cast::<JpegDecodeWorkspace>() };
        function(workspace)
    }
}

struct Workspaces {
    zip: &'static mut ZipValidationScratch,
    package: &'static mut DevicePackageScratch,
    frame_codec: &'static mut FrameCodecWorkspace,
    page: &'static mut BoundedPage,
    resource: &'static mut [u8; MAX_DEVICE_RESOURCE_BYTES],
    cover: &'static mut [u8; COVER_BYTES],
    shelf_covers: &'static mut [[u8; SHELF_COVER_BYTES]; VISIBLE_COVER_SLOTS],
}

#[derive(Clone, Copy)]
struct LoadedChapter {
    book: BookId,
    spine_index: usize,
    spine_count: usize,
    length: usize,
}

#[derive(Clone, Copy)]
struct ResumeRecord {
    magic: u32,
    kind: u32,
    primary: u32,
    secondary: u32,
    tertiary: u32,
    detail: u32,
    preferences: u32,
    checksum: u32,
}

#[derive(Clone, Copy)]
struct RetainedApp {
    resume: ResumePoint,
    preferences: ReaderPreferences,
}

const RESUME_MAGIC: u32 = 0x4257_5232;
const HOME_KIND: u32 = 1;
const BOOKS_KIND: u32 = 2;
const FILES_KIND: u32 = 3;
const SETTINGS_KIND: u32 = 4;
const READER_KIND: u32 = 5;

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut RETAINED_RESUME: [u32; 8] = [0; 8];

#[cfg(brewthink_previous_frame_storage = "host_ram")]
static DISPLAYED_FRAME: StaticCell<[u8; FRAME_BYTES]> = StaticCell::new();

const INPUT_EVENT_CAPACITY: usize = 32;
static INPUT_EVENTS: Channel<CriticalSectionRawMutex, ButtonEvent, INPUT_EVENT_CAPACITY> =
    Channel::new();
static STOP_INPUT: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static BATTERY_STATUS: Signal<CriticalSectionRawMutex, BatteryStatus> = Signal::new();
static POWER_PIN: Channel<CriticalSectionRawMutex, GPIO3<'static>, 1> = Channel::new();

#[embassy_executor::task]
pub async fn reader_input_task(mut inputs: X4InputHardware) {
    let mut debouncer = ButtonDebouncer::new();
    let mut battery = BatteryEstimator::new();
    let mut released_after_boot = false;

    'sampling: loop {
        if STOP_INPUT.try_take().is_some() {
            break;
        }

        match inputs.sample().await {
            Ok(sample) => {
                if let Some(status) = battery.observe(sample.battery_voltage(), sample.usb_state) {
                    BATTERY_STATUS.signal(status);
                }
                match decode_buttons(sample) {
                    Ok(buttons) => {
                        if !released_after_boot {
                            released_after_boot = buttons == PressedButtons::none();
                        } else if let Some(changes) = debouncer.update(buttons) {
                            for event in changes.events() {
                                if matches!(
                                    select(STOP_INPUT.wait(), INPUT_EVENTS.send(event)).await,
                                    Either::First(())
                                ) {
                                    break 'sampling;
                                }
                            }
                        }
                    }
                    Err(_) => debouncer.reject_sample(),
                }
            }
            Err(_) => debouncer.reject_sample(),
        }

        let next_sample = Timer::after(Duration::from_millis(20));
        if matches!(
            select(STOP_INPUT.wait(), next_sample).await,
            Either::First(())
        ) {
            break;
        }
    }

    POWER_PIN.send(inputs.into_power_pin()).await;
}

#[embassy_executor::task]
pub async fn reader_app_task(
    hardware: X4StorageHardware<'static>,
    mut control: UsbSerialJtagRx<'static, Blocking>,
    low_power: LPWR<'static>,
    wakeup_cause: SleepSource,
) {
    static STORE: StaticCell<DeviceStore> = StaticCell::new();
    static LIBRARY: StaticCell<DeviceLibrary> = StaticCell::new();
    static ZIP: StaticCell<ZipValidationScratch> = StaticCell::new();
    static PACKAGE: StaticCell<DevicePackageScratch> = StaticCell::new();
    static FRAME_CODEC: StaticCell<FrameCodecWorkspace> = StaticCell::new();
    static PAGE: StaticCell<BoundedPage> = StaticCell::new();
    static RESOURCE: StaticCell<[u8; MAX_DEVICE_RESOURCE_BYTES]> = StaticCell::new();
    static COVER: StaticCell<[u8; COVER_BYTES]> = StaticCell::new();
    static SHELF_COVERS: StaticCell<[[u8; SHELF_COVER_BYTES]; VISIBLE_COVER_SLOTS]> =
        StaticCell::new();

    let mut card = ReadOnlySdCard::new(hardware);
    if card.initialize().is_err() {
        stop("reader SD initialization failed").await;
    }
    let store = STORE.init(ReadOnlyFatBookStore::new(
        X4ReadOnlyFatBlockDevice::new(card),
        FixedTimeSource,
    ));
    let mut workspaces = Workspaces {
        zip: ZIP.init(ZipValidationScratch::new()),
        package: PACKAGE.init(DevicePackageScratch::new()),
        frame_codec: FRAME_CODEC.init(FrameCodecWorkspace::new()),
        page: PAGE.init_with(BoundedPage::new),
        resource: RESOURCE.init([0; MAX_DEVICE_RESOURCE_BYTES]),
        cover: COVER.init([0xFF; COVER_BYTES]),
        shelf_covers: SHELF_COVERS.init([[0xFF; SHELF_COVER_BYTES]; VISIBLE_COVER_SLOTS]),
    };
    let library = LIBRARY.init(DeviceLibrary::empty());
    if load_library(store, library, &mut workspaces).is_err() {
        stop("reader /BOOKS scan failed").await;
    }
    info!(
        "reader catalog ready: books={} cached_spines={} path_bytes={}",
        library.length, library.spine_path_count, library.spine_path_byte_length
    );

    let Some(profile) = X4DriveProfile::parse(X4_DRIVE_PROFILE) else {
        stop("reader X4 drive profile is invalid").await;
    };
    let Some(refresh_policy_mode) = RefreshPolicyMode::parse(DISPLAY_REFRESH) else {
        stop("reader display refresh policy is invalid").await;
    };
    let Some(mut panel) = initialize_panel(store, profile, refresh_policy_mode) else {
        stop("reader display initialization failed").await;
    };
    info!(
        "reader display configured: drive={=str} previous={=str} refresh={=str}",
        panel.display.drive_profile().name(),
        panel.display.previous_frame_storage().name(),
        panel.refresh_policy.mode().name()
    );
    let retained = read_resume().unwrap_or(RetainedApp {
        resume: ResumePoint::Home {
            selected: HomeItem::Books,
        },
        preferences: ReaderPreferences::default(),
    });
    let (mut app, first_effect) =
        App::from_resume(library.length, retained.preferences, retained.resume)
            .unwrap_or((App::new(library.length), AppEffect::RenderHome));
    if let Some(status) = BATTERY_STATUS.try_take() {
        app.set_battery(status);
    }
    let mut loaded = None;
    match run_effect(
        first_effect,
        &mut app,
        library,
        store,
        &mut panel,
        &mut workspaces,
        &mut loaded,
    ) {
        Ok(Some(resume)) => {
            enter_sleep(resume, app.preferences(), store, panel, low_power).await;
        }
        Ok(None) => {}
        Err(status) => stop(status).await,
    }
    if matches!(wakeup_cause, SleepSource::Gpio) {
        info!("reader resumed after GPIO3 deep-sleep wake");
    }

    let mut control_lines = ControlLineBuffer::new();
    esp_println::println!(
        "BREWCTL/1 READY version=1 width=480 height=800 bytes={}",
        FRAME_BYTES
    );
    write_control_status(&app);

    loop {
        if let Some(status) = BATTERY_STATUS.try_take() {
            app.set_battery(status);
        }
        if let Some(button) = poll_control(
            &mut control,
            &mut control_lines,
            &app,
            workspaces.frame_codec.frame(),
        ) {
            match (ReaderRuntime {
                app: &mut app,
                library,
                store,
                panel: &mut panel,
                workspaces: &mut workspaces,
                loaded: &mut loaded,
            })
            .run_input(InputSource::Usb, button)
            {
                Ok(Some(resume)) => {
                    enter_sleep(resume, app.preferences(), store, panel, low_power).await;
                }
                Ok(None) => {}
                Err(status) => stop(status).await,
            }
        }

        let next_control_poll = Timer::after(Duration::from_millis(20));
        let Either::First(event) = select(INPUT_EVENTS.receive(), next_control_poll).await else {
            continue;
        };
        if event.transition() != ButtonTransition::Pressed {
            continue;
        }

        match (ReaderRuntime {
            app: &mut app,
            library,
            store,
            panel: &mut panel,
            workspaces: &mut workspaces,
            loaded: &mut loaded,
        })
        .run_input(InputSource::Physical, event.button())
        {
            Ok(Some(resume)) => {
                enter_sleep(resume, app.preferences(), store, panel, low_power).await;
            }
            Ok(None) => {}
            Err(status) => stop(status).await,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum InputSource {
    Physical,
    Usb,
}

impl InputSource {
    const fn name(self) -> &'static str {
        match self {
            Self::Physical => "physical",
            Self::Usb => "usb",
        }
    }
}

struct ReaderRuntime<'a> {
    app: &'a mut App,
    library: &'a DeviceLibrary,
    store: &'a DeviceStore,
    panel: &'a mut ReaderDisplay,
    workspaces: &'a mut Workspaces,
    loaded: &'a mut Option<LoadedChapter>,
}

impl ReaderRuntime<'_> {
    fn run_input(
        &mut self,
        source: InputSource,
        button: Button,
    ) -> Result<Option<ResumePoint>, &'static str> {
        info!("reader button pressed: {=str}", button.name());
        esp_println::println!(
            "BREWCTL/1 EVENT source={} input={}",
            source.name(),
            button.name()
        );
        let result = run_effect(
            self.app.input(map_button(button)),
            self.app,
            self.library,
            self.store,
            self.panel,
            self.workspaces,
            self.loaded,
        );

        match result {
            Ok(resume) => {
                if source == InputSource::Usb {
                    write_control_status(self.app);
                    esp_println::println!("BREWCTL/1 DONE command=tap status=ok");
                }
                Ok(resume)
            }
            Err(status) => {
                if source == InputSource::Usb {
                    esp_println::println!("BREWCTL/1 ERROR command=tap reason=reader-operation");
                    esp_println::println!("BREWCTL/1 DONE command=tap status=error");
                }
                Err(status)
            }
        }
    }
}

fn poll_control(
    control: &mut UsbSerialJtagRx<'static, Blocking>,
    lines: &mut ControlLineBuffer,
    app: &App,
    frame: &[u8; FRAME_BYTES],
) -> Option<Button> {
    while let Ok(byte) = control.read_byte() {
        let Some(parsed) = lines.push(byte) else {
            continue;
        };
        match parsed {
            Ok(ControlCommand::Tap(button)) => return Some(button),
            Ok(ControlCommand::Status) => {
                write_control_status(app);
                esp_println::println!("BREWCTL/1 DONE command=status status=ok");
            }
            Ok(ControlCommand::Screen) => {
                write_control_screen(frame);
                esp_println::println!("BREWCTL/1 DONE command=screen status=ok");
            }
            Err(error) => {
                esp_println::println!("BREWCTL/1 ERROR command=parse reason={}", error.name());
                esp_println::println!("BREWCTL/1 DONE command=parse status=error");
            }
        }
    }
    None
}

fn write_control_status(app: &App) {
    match app.view() {
        AppView::Home(state) => {
            esp_println::println!(
                "BREWCTL/1 STATUS view=home selected={}",
                state.selected().index()
            );
        }
        AppView::Library => match app.library().selected() {
            Some(selected) => esp_println::println!(
                "BREWCTL/1 STATUS view=library selected={} books={}",
                selected.index(),
                app.library().book_count()
            ),
            None => esp_println::println!(
                "BREWCTL/1 STATUS view=library selected=none books={}",
                app.library().book_count()
            ),
        },
        AppView::Files(state) => match state.selected() {
            Some(selected) => esp_println::println!(
                "BREWCTL/1 STATUS view=files selected={} books={}",
                selected.index(),
                state.book_count()
            ),
            None => esp_println::println!(
                "BREWCTL/1 STATUS view=files selected=none books={}",
                state.book_count()
            ),
        },
        AppView::Settings(state) => {
            esp_println::println!(
                "BREWCTL/1 STATUS view=settings selected={} preferences={}",
                state.selected().index(),
                state.draft().packed()
            );
        }
        AppView::Loading => {
            esp_println::println!("BREWCTL/1 STATUS view=loading");
        }
        AppView::Reader(session) => {
            let location = session.location();
            esp_println::println!(
                "BREWCTL/1 STATUS view=reader book={} spine={} page={} pages={}",
                location.book().index(),
                location.spine_index(),
                location.page_index(),
                location.page_count()
            );
        }
        AppView::Error { book, origin: _ } => {
            esp_println::println!("BREWCTL/1 STATUS view=error book={}", book.index());
        }
        AppView::Sleeping { .. } => {
            esp_println::println!("BREWCTL/1 STATUS view=sleeping");
        }
    }
}

fn write_control_screen(frame: &[u8; FRAME_BYTES]) {
    let crc32 = crc32fast::hash(frame);
    esp_println::println!(
        "BREWCTL/1 SCREEN width=480 height=800 bytes={} crc32={:08x}",
        frame.len(),
        crc32
    );
    esp_println::Printer::write_bytes(frame);
    esp_println::Printer::write_bytes(b"\n");
}

fn load_library(
    store: &DeviceStore,
    library: &mut DeviceLibrary,
    workspaces: &mut Workspaces,
) -> Result<(), ()> {
    let catalog = store.scan::<MAX_DEVICE_BOOKS>().map_err(|_| ())?;
    for file in catalog.books().copied() {
        let inflate = workspaces.frame_codec.prepare_inflate();
        let reader = match store.open_reader(file) {
            Ok(reader) => reader,
            Err(_) => continue,
        };
        let book = match DeviceEpub::open(
            reader,
            workspaces.zip,
            workspaces.package,
            inflate,
            workspaces.resource,
        ) {
            Ok(book) => book,
            Err(_) => continue,
        };
        let publication = book.publication();
        let Ok(title) = FixedString::try_from_str(publication.title()) else {
            continue;
        };
        let Ok(creator) = FixedString::try_from_str(publication.creator()) else {
            continue;
        };
        let cover_path = match publication
            .cover_path()
            .map(FixedString::try_from_str)
            .transpose()
        {
            Ok(path) => path,
            Err(_) => continue,
        };
        let Ok(spine_count) = u8::try_from(publication.spine_len()) else {
            continue;
        };
        let index = library.length;
        let book_id = BookId::new(index);
        library.cache_spine_paths(book_id, publication);
        library.files[index] = Some(file);
        library.titles[index] = title;
        library.creators[index] = creator;
        library.cover_paths[index] = cover_path;
        library.spine_counts[index] = spine_count;
        library.length += 1;
    }
    Ok(())
}

fn initialize_panel(
    store: &DeviceStore,
    profile: X4DriveProfile,
    refresh_policy_mode: RefreshPolicyMode,
) -> Option<ReaderDisplay> {
    store.with_device(|device| {
        device.with_hardware(|hardware| {
            let mut bus = hardware.display_bus().ok()?;
            let controller = Ssd1677::with_profile(profile).initialize(&mut bus).ok()?;
            #[cfg(brewthink_previous_frame_storage = "host_ram")]
            let display = BufferedDisplay::with_host_ram(
                controller,
                DISPLAYED_FRAME.init_with(|| [0xFF; FRAME_BYTES]),
                Rotation::Degrees270,
            );
            #[cfg(brewthink_previous_frame_storage = "controller_ram")]
            let display = BufferedDisplay::with_controller_ram(controller, Rotation::Degrees270);
            Some(ReaderDisplay {
                display,
                refresh_policy: RefreshPolicy::new(refresh_policy_mode),
            })
        })
    })
}

#[inline(always)]
fn run_effect(
    mut effect: AppEffect,
    app: &mut App,
    library: &DeviceLibrary,
    store: &DeviceStore,
    panel: &mut ReaderDisplay,
    workspaces: &mut Workspaces,
    loaded: &mut Option<LoadedChapter>,
) -> Result<Option<ResumePoint>, &'static str> {
    loop {
        effect = match effect {
            AppEffect::None => return Ok(None),
            AppEffect::RenderHome => {
                render_home_frame(app, workspaces.frame_codec.frame())?;
                refresh(store, panel, workspaces.frame_codec.frame())?;
                return Ok(None);
            }
            AppEffect::RenderLibrary => {
                esp_println::println!("BREWCTL/1 LOG stage=render-library state=start");
                render_library(app, library, store, workspaces)?;
                esp_println::println!("BREWCTL/1 LOG stage=render-library state=frame-ready");
                refresh(store, panel, workspaces.frame_codec.frame())?;
                esp_println::println!("BREWCTL/1 LOG stage=render-library state=done");
                info!(
                    "reader shelf refreshed: selected={}",
                    app.library().selected().map_or(usize::MAX, BookId::index)
                );
                return Ok(None);
            }
            AppEffect::RenderFiles => {
                render_files_frame(app, library, workspaces.frame_codec.frame())?;
                refresh(store, panel, workspaces.frame_codec.frame())?;
                return Ok(None);
            }
            AppEffect::RenderSettings => {
                render_settings_frame(app, workspaces.frame_codec.frame())?;
                refresh(store, panel, workspaces.frame_codec.frame())?;
                return Ok(None);
            }
            AppEffect::LoadChapter {
                book,
                spine_index,
                target: _,
            } => {
                esp_println::println!(
                    "BREWCTL/1 LOG stage=load-chapter state=start book={} spine={}",
                    book.index(),
                    spine_index
                );
                let next = match load_chapter(book, spine_index, library, store, workspaces) {
                    Ok(chapter) => {
                        *loaded = Some(chapter);
                        match layout_xhtml_page_into(
                            &workspaces.resource[..chapter.length],
                            0,
                            app.preferences(),
                            workspaces.page,
                        ) {
                            Ok(()) => app
                                .chapter_loaded(chapter.spine_count, workspaces.page.page_count())
                                .map_err(|_| "reader application state rejected chapter")?,
                            Err(_) => app
                                .chapter_failed()
                                .map_err(|_| "reader application state rejected layout failure")?,
                        }
                    }
                    Err(_) => app
                        .chapter_failed()
                        .map_err(|_| "reader application state rejected failure")?,
                };
                esp_println::println!(
                    "BREWCTL/1 LOG stage=load-chapter state=done book={} spine={}",
                    book.index(),
                    spine_index
                );
                next
            }
            AppEffect::RenderReader(location) => {
                esp_println::println!(
                    "BREWCTL/1 LOG stage=render-reader state=start book={} spine={} page={}",
                    location.book().index(),
                    location.spine_index(),
                    location.page_index()
                );
                let chapter = loaded.ok_or("reader chapter was not loaded")?;
                if chapter.book != location.book() || chapter.spine_index != location.spine_index()
                {
                    return Err("reader chapter cache mismatch");
                }
                let xhtml = &workspaces.resource[..chapter.length];
                render_page(
                    app,
                    location,
                    library,
                    xhtml,
                    workspaces.page,
                    workspaces.frame_codec.frame(),
                )?;
                refresh(store, panel, workspaces.frame_codec.frame())?;
                esp_println::println!(
                    "BREWCTL/1 LOG stage=render-reader state=done book={} spine={} page={}",
                    location.book().index(),
                    location.spine_index(),
                    location.page_index()
                );
                info!(
                    "reader page refreshed: book={} spine={} page={} pages={}",
                    location.book().index(),
                    location.spine_index(),
                    location.page_index(),
                    location.page_count()
                );
                return Ok(None);
            }
            AppEffect::RenderError { book } => {
                esp_println::println!("BREWCTL/1 LOG stage=render-error state=start");
                render_error(app, book, library, workspaces.frame_codec.frame())?;
                refresh(store, panel, workspaces.frame_codec.frame())?;
                esp_println::println!("BREWCTL/1 LOG stage=render-error state=done");
                return Ok(None);
            }
            AppEffect::RenderSleep { resume } => {
                esp_println::println!("BREWCTL/1 LOG stage=render-sleep state=start");
                render_sleep_frame(resume, app.battery(), library, store, workspaces)?;
                refresh(store, panel, workspaces.frame_codec.frame())?;
                esp_println::println!("BREWCTL/1 LOG stage=render-sleep state=done");
                info!("reader retained sleep frame refreshed");
                app.sleep_frame_ready()
                    .map_err(|_| "reader application state rejected sleep frame")?
            }
            AppEffect::EnterDeepSleep { resume } => return Ok(Some(resume)),
        };
    }
}

#[inline(always)]
fn load_chapter(
    selected: BookId,
    spine_index: usize,
    library: &DeviceLibrary,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<LoadedChapter, ()> {
    let file = library.file(selected).ok_or(())?;
    let inflate = workspaces.frame_codec.prepare_inflate();
    let reader = store.open_reader(file).map_err(|_| ())?;
    let (spine_count, length) = if let Some(path) = library.spine_path(selected, spine_index) {
        let archive = StreamingZip::open(reader, workspaces.zip).map_err(|_| ())?;
        let entry = archive.find(path).map_err(|_| ())?;
        if entry.uncompressed_size() as usize > workspaces.resource.len() {
            return Err(());
        }
        let length = archive
            .read_entry(entry, workspaces.resource, inflate)
            .map_err(|_| ())?;
        (library.spine_count(selected), length)
    } else {
        let book = DeviceEpub::open(
            reader,
            workspaces.zip,
            workspaces.package,
            inflate,
            workspaces.resource,
        )
        .map_err(|_| ())?;
        let spine_count = book.publication().spine_len();
        let length = book
            .read_spine(spine_index, workspaces.resource, inflate)
            .map_err(|_| ())?;
        (spine_count, length)
    };
    Ok(LoadedChapter {
        book: selected,
        spine_index,
        spine_count,
        length,
    })
}

fn decode_book_cover(
    selected: BookId,
    library: &DeviceLibrary,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<bool, &'static str> {
    let Some(path) = library.cover_path(selected) else {
        return Ok(false);
    };
    esp_println::println!(
        "BREWCTL/1 LOG stage=cover state=start book={}",
        selected.index()
    );
    let file = library.file(selected).ok_or("reader book is missing")?;
    let inflate = workspaces.frame_codec.prepare_inflate();
    let reader = store
        .open_reader(file)
        .map_err(|_| "reader cover file open failed")?;
    let archive = StreamingZip::open(reader, workspaces.zip)
        .map_err(|_| "reader cover archive open failed")?;
    esp_println::println!(
        "BREWCTL/1 LOG stage=cover state=archive-open book={}",
        selected.index()
    );
    let entry = archive
        .find(path)
        .map_err(|_| "reader cover entry is missing")?;
    esp_println::println!(
        "BREWCTL/1 LOG stage=cover state=entry-found book={} compressed={} uncompressed={}",
        selected.index(),
        entry.compressed_size(),
        entry.uncompressed_size()
    );
    if !encoded_cover_fits(entry.compressed_size(), entry.uncompressed_size()) {
        esp_println::println!(
            "BREWCTL/1 LOG stage=cover state=skipped book={} reason=encoded-size",
            selected.index()
        );
        return Ok(false);
    }
    let length = archive
        .read_entry(entry, workspaces.resource, inflate)
        .map_err(|_| "reader cover read failed")?;
    let encoded = &workspaces.resource[..length];
    let output = &mut *workspaces.cover;
    let decoded = if encoded.starts_with(b"\x89PNG\r\n\x1a\n") {
        esp_println::println!(
            "BREWCTL/1 LOG stage=cover state=decode-start book={} format=png bytes={}",
            selected.index(),
            length
        );
        workspaces
            .frame_codec
            .with_png(|png| decode_png_cover(encoded, output, png))
    } else if encoded.starts_with(&[0xFF, 0xD8]) {
        esp_println::println!(
            "BREWCTL/1 LOG stage=cover state=decode-start book={} format=jpeg bytes={}",
            selected.index(),
            length
        );
        workspaces
            .frame_codec
            .with_jpeg(|jpeg| decode_jpeg_cover(encoded, output, jpeg))
    } else {
        return Ok(false);
    };
    decoded.map_err(|_| "reader cover decode failed")?;
    esp_println::println!(
        "BREWCTL/1 LOG stage=cover state=done book={}",
        selected.index()
    );
    Ok(true)
}

fn render_home_frame(app: &App, frame: &mut [u8; FRAME_BYTES]) -> Result<(), &'static str> {
    let mut image = MonochromeImage::new(frame_size(), frame)
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_home(app.home(), app.battery(), &mut image).map_err(|_| "reader home render failed")
}

fn render_files_frame(
    app: &App,
    library: &DeviceLibrary,
    frame: &mut [u8; FRAME_BYTES],
) -> Result<(), &'static str> {
    let mut files = [FileItem::new("", 0); MAX_DEVICE_BOOKS];
    for (index, file) in files[..library.length].iter_mut().enumerate() {
        let book = BookId::new(index);
        *file = FileItem::new(library.file_name(book), library.file_size(book));
    }
    let mut image = MonochromeImage::new(frame_size(), frame)
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_files(
        app.files(),
        &files[..library.length],
        app.battery(),
        &mut image,
    )
    .map_err(|_| "reader files render failed")
}

fn render_settings_frame(app: &App, frame: &mut [u8; FRAME_BYTES]) -> Result<(), &'static str> {
    let AppView::Settings(settings) = app.view() else {
        return Err("reader settings render requested outside settings");
    };
    let mut image = MonochromeImage::new(frame_size(), frame)
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_settings(settings, app.battery(), &mut image)
        .map_err(|_| "reader settings render failed")
}

fn render_library(
    app: &App,
    library: &DeviceLibrary,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), &'static str> {
    let mut books = [ShelfBook::new("", "", None); MAX_DEVICE_BOOKS];
    for (index, book) in books[..library.length].iter_mut().enumerate() {
        *book = ShelfBook::new(
            library.titles[index].as_str(),
            library.creators[index].as_str(),
            None,
        );
    }
    let visible = app.library().visible_range();
    let selected = app.library().selected().map(BookId::index);
    let mut decoded = [false; VISIBLE_COVER_SLOTS];
    for (slot, index) in visible.clone().enumerate() {
        if Some(index) == selected {
            continue;
        }
        decoded[slot] =
            decode_book_cover(BookId::new(index), library, store, workspaces).unwrap_or(false);
        if decoded[slot] {
            downsample_cover(workspaces.cover, &mut workspaces.shelf_covers[slot]);
        }
    }
    let selected_full = selected.is_some_and(|index| {
        if !visible.contains(&index) {
            return false;
        }
        let slot = index - visible.start;
        decoded[slot] =
            decode_book_cover(BookId::new(index), library, store, workspaces).unwrap_or(false);
        if decoded[slot] {
            downsample_cover(workspaces.cover, &mut workspaces.shelf_covers[slot]);
        }
        decoded[slot]
    });
    let covers = &*workspaces.shelf_covers;
    let full_cover = &*workspaces.cover;
    let mut image = MonochromeImage::new(frame_size(), workspaces.frame_codec.frame())
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_shelf(
        app.library(),
        &books[..library.length],
        app.battery(),
        &mut image,
    )
    .map_err(|_| "reader shelf render failed")?;
    for (slot, index) in visible.enumerate() {
        if decoded[slot] {
            let cover = if selected_full && Some(index) == selected {
                bitmap(full_cover)
            } else {
                shelf_bitmap(&covers[slot])
            };
            render_shelf_cover(app.library(), index, cover, &mut image)
                .map_err(|_| "reader shelf cover render failed")?;
        }
    }
    Ok(())
}

fn render_page(
    app: &App,
    location: ReadingLocation,
    library: &DeviceLibrary,
    xhtml: &[u8],
    page: &mut BoundedPage,
    frame: &mut [u8; FRAME_BYTES],
) -> Result<(), &'static str> {
    layout_xhtml_page_into(xhtml, location.page_index(), app.preferences(), page)
        .map_err(|_| "reader requested page layout failed")?;
    let mut lines = [ReaderLine::new("", ReaderStyle::Body); MAX_PAGE_LINES];
    let mut line_count = 0;
    for line in page.lines() {
        lines[line_count] = ReaderLine::new(line.text(), line.style());
        line_count += 1;
    }
    let view = ReaderView::new(
        library.title(location.book()),
        page.chapter_title(),
        &lines[..line_count],
        location,
        app.preferences(),
        app.battery(),
    );
    let mut image = MonochromeImage::new(frame_size(), frame)
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_reader(view, &mut image).map_err(|_| "reader page render failed")
}

fn render_sleep_frame(
    resume: ResumePoint,
    battery: BatteryStatus,
    library: &DeviceLibrary,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), &'static str> {
    let selected = match resume {
        ResumePoint::Books { selected } | ResumePoint::Files { selected } => selected,
        ResumePoint::Reader { book, .. } => Some(book),
        ResumePoint::Home { .. } | ResumePoint::Settings { .. } => None,
    };
    let mut status = FixedString::<64>::new();
    match resume {
        ResumePoint::Reader {
            spine_index,
            page_index,
            ..
        } => write!(
            status,
            "SAVED  CHAPTER {}  PAGE {}",
            spine_index + 1,
            page_index + 1
        )
        .map_err(|_| "reader sleep status overflowed")?,
        ResumePoint::Home { .. } => status
            .push_str("HOME POSITION SAVED")
            .map_err(|_| "reader sleep status overflowed")?,
        ResumePoint::Books { .. } => status
            .push_str("BOOKS POSITION SAVED")
            .map_err(|_| "reader sleep status overflowed")?,
        ResumePoint::Files { .. } => status
            .push_str("FILES POSITION SAVED")
            .map_err(|_| "reader sleep status overflowed")?,
        ResumePoint::Settings { .. } => status
            .push_str("SETTINGS POSITION SAVED")
            .map_err(|_| "reader sleep status overflowed")?,
    }
    let has_cover = selected
        .is_some_and(|book| decode_book_cover(book, library, store, workspaces).unwrap_or(false));
    let (title, creator) = selected.map_or(("Brewthink", ""), |book| {
        (library.title(book), library.creator(book))
    });
    let cover = has_cover.then(|| bitmap(workspaces.cover));
    let mut image = MonochromeImage::new(frame_size(), workspaces.frame_codec.frame())
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_sleep(
        SleepView::new(title, creator, status.as_str(), cover, battery),
        &mut image,
    )
    .map_err(|_| "reader sleep frame render failed")
}

fn render_error(
    app: &App,
    book: BookId,
    library: &DeviceLibrary,
    frame: &mut [u8; FRAME_BYTES],
) -> Result<(), &'static str> {
    let mut image = MonochromeImage::new(frame_size(), frame)
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_reader_error(
        library.title(book),
        "This EPUB or chapter could not be opened.",
        app.battery(),
        &mut image,
    )
    .map_err(|_| "reader error frame render failed")
}

fn refresh(
    store: &DeviceStore,
    panel: &mut ReaderDisplay,
    bytes: &[u8; FRAME_BYTES],
) -> Result<(), &'static str> {
    let mode = panel.refresh_policy.requested_mode();
    let applied = store.with_device(|device| {
        device.with_hardware(|hardware| {
            let mut bus = hardware
                .display_bus()
                .map_err(|_| "reader display session failed")?;
            panel
                .display
                .refresh(&mut bus, bytes, mode)
                .map_err(|_| "reader display refresh failed")
        })
    })?;
    panel.refresh_policy.commit(applied);
    info!(
        "reader display refreshed: drive={=str} previous={=str} policy={=str} requested={=str} applied={=str}",
        panel.display.drive_profile().name(),
        panel.display.previous_frame_storage().name(),
        panel.refresh_policy.mode().name(),
        mode.name(),
        applied.name()
    );
    Ok(())
}

async fn enter_sleep(
    resume: ResumePoint,
    preferences: ReaderPreferences,
    store: &'static DeviceStore,
    panel: ReaderDisplay,
    low_power: LPWR<'static>,
) -> ! {
    write_resume(resume, preferences);
    let sleep_result = store.with_device(move |device| {
        device.with_hardware(move |hardware| {
            let mut bus = hardware
                .display_bus()
                .map_err(|_| "reader display sleep session failed")?;
            panel
                .display
                .enter_deep_sleep(&mut bus)
                .map_err(|_| "reader display sleep command failed")
        })
    });
    if let Err(status) = sleep_result {
        stop(status).await;
    }
    STOP_INPUT.signal(());
    let mut power = POWER_PIN.receive().await;
    loop {
        let released = {
            let input = Input::new(power.reborrow(), InputConfig::default().with_pull(Pull::Up));
            input.is_high()
        };
        if released {
            break;
        }
        Timer::after(Duration::from_millis(20)).await;
    }
    Timer::after(Duration::from_millis(100)).await;
    info!("reader entering deep sleep: wake_gpio=3 wake_level=low");
    embedded_hal::delay::DelayNs::delay_ms(&mut Delay::new(), 50);
    let mut rtc = Rtc::new(low_power);
    let wakeup_pins: &mut [(&mut dyn RtcPinWithResistors, WakeupLevel)] =
        &mut [(&mut power, WakeupLevel::Low)];
    let wake = RtcioWakeupSource::new(wakeup_pins);
    rtc.sleep_deep(&[&wake]);
}

fn map_button(button: Button) -> AppInput {
    match button {
        Button::Back => AppInput::Back,
        Button::Confirm => AppInput::Confirm,
        Button::Left => AppInput::Move(Direction::Left),
        Button::Right => AppInput::Move(Direction::Right),
        Button::Up => AppInput::Move(Direction::Up),
        Button::Down => AppInput::Move(Direction::Down),
        Button::Power => AppInput::Power,
    }
}

fn read_resume() -> Option<RetainedApp> {
    // SAFETY: the single-core reader accesses this fixed RTC record only from its task.
    let words = unsafe { core::ptr::read_volatile(&raw const RETAINED_RESUME) };
    let record = ResumeRecord {
        magic: words[0],
        kind: words[1],
        primary: words[2],
        secondary: words[3],
        tertiary: words[4],
        detail: words[5],
        preferences: words[6],
        checksum: words[7],
    };
    if record.magic != RESUME_MAGIC || record.checksum != resume_checksum(record) {
        return None;
    }
    let preferences = ReaderPreferences::from_packed(record.preferences)?;
    let selected = || {
        if record.primary == u32::MAX {
            Some(None)
        } else {
            Some(Some(BookId::new(usize::try_from(record.primary).ok()?)))
        }
    };
    let resume = match record.kind {
        HOME_KIND => ResumePoint::Home {
            selected: HomeItem::from_index(usize::try_from(record.primary).ok()?)?,
        },
        BOOKS_KIND => ResumePoint::Books {
            selected: selected()?,
        },
        FILES_KIND => ResumePoint::Files {
            selected: selected()?,
        },
        SETTINGS_KIND => ResumePoint::Settings {
            selected: SettingsItem::from_index(usize::try_from(record.primary).ok()?)?,
            draft: ReaderPreferences::from_packed(record.detail)?,
        },
        READER_KIND => ResumePoint::Reader {
            book: BookId::new(usize::try_from(record.primary).ok()?),
            spine_index: usize::try_from(record.secondary).ok()?,
            page_index: usize::try_from(record.tertiary).ok()?,
            origin: BookOrigin::from_index(usize::try_from(record.detail).ok()?)?,
        },
        _ => return None,
    };
    Some(RetainedApp {
        resume,
        preferences,
    })
}

fn write_resume(resume: ResumePoint, preferences: ReaderPreferences) {
    let (kind, primary, secondary, tertiary, detail) = match resume {
        ResumePoint::Home { selected } => (HOME_KIND, selected.index() as u32, 0, 0, 0),
        ResumePoint::Books { selected } => (BOOKS_KIND, packed_book(selected), 0, 0, 0),
        ResumePoint::Files { selected } => (FILES_KIND, packed_book(selected), 0, 0, 0),
        ResumePoint::Settings { selected, draft } => {
            (SETTINGS_KIND, selected.index() as u32, 0, 0, draft.packed())
        }
        ResumePoint::Reader {
            book,
            spine_index,
            page_index,
            origin,
        } => (
            READER_KIND,
            u32::try_from(book.index()).unwrap_or(u32::MAX),
            u32::try_from(spine_index).unwrap_or(u32::MAX),
            u32::try_from(page_index).unwrap_or(u32::MAX),
            origin.index() as u32,
        ),
    };
    let mut record = ResumeRecord {
        magic: RESUME_MAGIC,
        kind,
        primary,
        secondary,
        tertiary,
        detail,
        preferences: preferences.packed(),
        checksum: 0,
    };
    record.checksum = resume_checksum(record);
    let words = [
        record.magic,
        record.kind,
        record.primary,
        record.secondary,
        record.tertiary,
        record.detail,
        record.preferences,
        record.checksum,
    ];
    // SAFETY: the single-core reader is the sole writer before entering deep sleep.
    unsafe { core::ptr::write_volatile(&raw mut RETAINED_RESUME, words) };
}

fn packed_book(book: Option<BookId>) -> u32 {
    book.and_then(|book| u32::try_from(book.index()).ok())
        .unwrap_or(u32::MAX)
}

fn resume_checksum(record: ResumeRecord) -> u32 {
    [
        record.magic,
        record.kind,
        record.primary,
        record.secondary,
        record.tertiary,
        record.detail,
        record.preferences,
    ]
    .into_iter()
    .fold(0x811C_9DC5, |hash, value| {
        (hash ^ value).wrapping_mul(0x0100_0193)
    })
}

fn downsample_cover(source: &[u8; COVER_BYTES], output: &mut [u8; SHELF_COVER_BYTES]) {
    let source = bitmap(source);
    output.fill(0xFF);
    for y in 0..SHELF_COVER_HEIGHT {
        for x in 0..SHELF_COVER_WIDTH {
            let source_x = x * 2;
            let source_y = y * 2;
            let black = usize::from(source.pixel_is_black(source_x, source_y))
                + usize::from(source.pixel_is_black(source_x + 1, source_y))
                + usize::from(source.pixel_is_black(source_x, source_y + 1))
                + usize::from(source.pixel_is_black(source_x + 1, source_y + 1));
            if black >= 2 {
                let pixel = y * SHELF_COVER_WIDTH + x;
                output[pixel / 8] &= !(0x80 >> (pixel % 8));
            }
        }
    }
}

fn shelf_bitmap(bytes: &[u8; SHELF_COVER_BYTES]) -> MonochromeBitmap<'_> {
    MonochromeBitmap::new(
        Size::new(SHELF_COVER_WIDTH, SHELF_COVER_HEIGHT)
            .expect("the shelf cover dimensions are non-zero"),
        bytes,
    )
    .expect("the shelf cover buffer matches its dimensions")
}

fn frame_size() -> Size {
    Size::new(480, 800).expect("the X4 frame dimensions are non-zero")
}

async fn stop(status: &'static str) -> ! {
    info!("{=str}; holding without retry", status);
    loop {
        core::future::pending::<()>().await;
    }
}
