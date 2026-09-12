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
use static_cell::{ConstStaticCell, StaticCell};

use self::retained_resume::RtcResume;

use crate::{
    app::{
        App, AppEffect, AppInput, AppPreferences, AppView, BookId, Direction, HomeItem, ImageId,
        ReadingLocation, ResumePoint, SettingsItem, SleepScreenMode, SleepScreenSource,
    },
    bounded_layout::{BoundedPage, MAX_PAGE_LINES, layout_xhtml_page_into},
    bounded_xml::FixedString,
    cover::{
        COVER_BYTES, CoverDecodeWorkspace, JpegDecodeWorkspace, MAX_ENCODED_COVER_BYTES,
        SHELF_COVER_BYTES, bitmap, decode_jpeg_cover, decode_png_cover, downsample_cover,
        encoded_cover_fits, shelf_bitmap,
    },
    device_epub::{
        DeviceEpub, DevicePackageScratch, DevicePublication, MAX_DEVICE_PATH_BYTES,
        MAX_DEVICE_RESOURCE_BYTES,
    },
    display::{
        framebuffer::{FRAME_BYTES as MONO_FRAME_BYTES, Rotation},
        ssd1677::{BufferedDisplay, RefreshPolicy, RefreshPolicyMode, Ssd1677, X4DriveProfile},
    },
    files::{FileItem, FileKind},
    image::{Dither, PackedBitmap, PackedImage, READER_DEPTH, RenderOptions, ScaleMode, Size},
    image_decoder::{ImageFormat, decode_jpeg, decode_png},
    image_viewer::render_image_viewer,
    input::{
        Button, ButtonDebouncer, ButtonEvent, ButtonTransition, PressedButtons,
        control::{ControlCommand, ControlLineBuffer},
    },
    library::ShelfBook,
    power::{BatteryEstimator, BatteryLevel, BatteryStatus},
    reader::{ReaderLine, ReaderStyle, ReaderView},
    reader_orchestration::{self, ChapterPages, ReaderIo, Rendered, Startup, StartupStage},
    settings::CustomImagePreview,
    sleep::SleepView,
    storage::{
        BookCatalog, BookFile, FatStorage, ImageFile, MAX_DEVICE_IMAGE_BYTES, ReadOnlySdCard,
    },
    transfer::{FileTransfer, UploadRequest},
    ui::{AppFrame, render_app},
    x4::{X4FatBlockDevice, X4InputHardware, X4StorageHardware, decode_buttons},
    zip_stream::{InflateWorkspace, StreamingZip, ZipValidationScratch},
};

type FileError = embedded_sdmmc::Error<crate::x4::X4FatBlockDeviceError>;

#[derive(Debug)]
enum ReaderError {
    File(FileError),
    Card(crate::storage::SdError<crate::x4::X4StorageError>),
    AppData(crate::storage::AppDataError<crate::x4::X4FatBlockDeviceError>),
    Epub(crate::device_epub::DeviceEpubError<FileError>),
    Archive(crate::zip_stream::ZipError<FileError>),
    Layout(crate::bounded_layout::LayoutError),
    BookMissing,
    ResourceTooLarge,
    Render(&'static str),
}

impl core::fmt::Display for ReaderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::File(error) => write!(f, "{error}"),
            Self::Card(error) => write!(f, "{error:?}"),
            Self::AppData(error) => write!(f, "{error}"),
            Self::Epub(error) => write!(f, "{error:?}"),
            Self::Archive(error) => write!(f, "{error:?}"),
            Self::Layout(error) => write!(f, "{error}"),
            Self::BookMissing => f.write_str("book is missing"),
            Self::ResourceTooLarge => f.write_str("chapter exceeds resource capacity"),
            Self::Render(error) => f.write_str(error),
        }
    }
}

const MAX_DEVICE_BOOKS: usize = 16;
const MAX_DEVICE_IMAGES: usize = 16;
const MAX_DEVICE_FILES: usize = MAX_DEVICE_BOOKS + MAX_DEVICE_IMAGES;
const MAX_CACHED_SPINE_PATHS: usize = 64;
const MAX_CACHED_SPINE_PATH_BYTES: usize = 2 * 1024;
const VISIBLE_COVER_SLOTS: usize = 4;
const FRAME_BYTES: usize = MONO_FRAME_BYTES * READER_DEPTH.bits();
const UPLOAD_CHUNK_BYTES: usize = 4 * 1024;
const UPLOAD_IDLE_POLLS: usize = 1_500;
const IMAGE_DECODER_BYTES: usize = MAX_DEVICE_RESOURCE_BYTES - MAX_DEVICE_IMAGE_BYTES;
const _: () = {
    assert!(core::mem::size_of::<CoverDecodeWorkspace>() <= FRAME_BYTES);
    assert!(core::mem::size_of::<JpegDecodeWorkspace>() <= FRAME_BYTES);
    assert!(core::mem::size_of::<CoverDecodeWorkspace>() <= IMAGE_DECODER_BYTES);
    assert!(core::mem::size_of::<JpegDecodeWorkspace>() <= IMAGE_DECODER_BYTES);
};
const _: () = assert!(
    MAX_ENCODED_COVER_BYTES as usize + (VISIBLE_COVER_SLOTS - 1) * SHELF_COVER_BYTES
        <= MAX_DEVICE_RESOURCE_BYTES
);
const CONTENT_BYTES: usize = if core::mem::size_of::<BoundedPage>() > COVER_BYTES {
    core::mem::size_of::<BoundedPage>()
} else {
    COVER_BYTES
};
const X4_DRIVE_PROFILE: &str = match option_env!("BREWTHINK_X4_DRIVE_PROFILE") {
    Some(profile) => profile,
    None => "stock-parity",
};
const DISPLAY_REFRESH: &str = match option_env!("BREWTHINK_DISPLAY_REFRESH") {
    Some(mode) => mode,
    None => "automatic",
};
type DeviceStore = FatStorage<X4FatBlockDevice<'static>, FixedTimeSource>;

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

type DeviceImages = reader_orchestration::ReaderImages<MAX_DEVICE_IMAGES>;

#[derive(Clone, Copy)]
struct DeviceCatalogs<'a> {
    library: &'a DeviceLibrary,
    images: &'a DeviceImages,
}

struct FrameCodecWorkspace {
    storage: crate::scratch::Scratch<FRAME_BYTES>,
}

struct EpubWorkspace {
    inflate: InflateWorkspace,
    publication: DevicePublication,
}

impl EpubWorkspace {
    unsafe fn initialize(storage: *mut Self) {
        // SAFETY: each initializer writes its field in the aligned enclosing allocation.
        unsafe {
            InflateWorkspace::initialize_in_place(core::ptr::addr_of_mut!((*storage).inflate));
            DevicePublication::initialize_in_place(core::ptr::addr_of_mut!((*storage).publication));
        }
    }
}

impl FrameCodecWorkspace {
    const fn new() -> Self {
        Self {
            storage: crate::scratch::Scratch::new(),
        }
    }

    fn frame(&mut self) -> &mut [u8; FRAME_BYTES] {
        self.storage.bytes()
    }

    fn prepare_inflate(&mut self) -> &mut InflateWorkspace {
        // SAFETY: the inflater initializer establishes a valid Raw-format workspace.
        unsafe {
            self.storage
                .initialize(InflateWorkspace::initialize_in_place)
        }
    }

    fn prepare_epub(&mut self) -> (&mut InflateWorkspace, &mut DevicePublication) {
        // SAFETY: both fields are initialized before the enclosing value is exposed.
        let workspace = unsafe { self.storage.initialize(EpubWorkspace::initialize) };
        (&mut workspace.inflate, &mut workspace.publication)
    }

    fn with_png<R>(&mut self, function: impl FnOnce(&mut CoverDecodeWorkspace) -> R) -> R {
        let workspace = CoverDecodeWorkspace::in_buffer(self.storage.bytes())
            .expect("the frame allocation fits the PNG workspace");
        function(workspace)
    }

    fn with_jpeg<R>(&mut self, function: impl FnOnce(&mut JpegDecodeWorkspace) -> R) -> R {
        let workspace = JpegDecodeWorkspace::in_buffer(self.storage.bytes())
            .expect("the frame allocation fits the JPEG workspace");
        function(workspace)
    }
}

struct BookNavigation {
    book: Option<BookId>,
    titles: [FixedString<{ crate::navigation::CHAPTER_TITLE_BYTES }>;
        crate::device_epub::MAX_DEVICE_SPINE_ITEMS],
}

impl BookNavigation {
    const fn new() -> Self {
        Self {
            book: None,
            titles: [FixedString::new(); crate::device_epub::MAX_DEVICE_SPINE_ITEMS],
        }
    }
}

struct ContentWorkspace {
    storage: crate::scratch::Scratch<CONTENT_BYTES>,
}

impl ContentWorkspace {
    const fn new() -> Self {
        Self {
            storage: crate::scratch::Scratch::new(),
        }
    }

    fn prepare_catalog(&mut self) -> &mut BookCatalog<MAX_DEVICE_BOOKS> {
        // SAFETY: the catalog initializer writes all entries and counters in place.
        unsafe { self.storage.initialize(BookCatalog::initialize_in_place) }
    }

    fn prepare_page(&mut self) -> &mut BoundedPage {
        // SAFETY: the page initializer writes every field in place.
        unsafe { self.storage.initialize(BoundedPage::initialize_in_place) }
    }

    fn cover(&mut self) -> &mut [u8; COVER_BYTES] {
        (&mut self.storage.bytes()[..COVER_BYTES])
            .try_into()
            .expect("content storage fits the cover")
    }
}

struct Workspaces {
    navigation: &'static mut BookNavigation,
    zip: &'static mut ZipValidationScratch,
    package: &'static mut DevicePackageScratch,
    frame_codec: &'static mut FrameCodecWorkspace,
    content: &'static mut ContentWorkspace,
    resource: &'static mut [u8; MAX_DEVICE_RESOURCE_BYTES],
}

#[derive(Clone, Copy)]
struct LoadedChapter {
    book: BookId,
    spine_index: usize,
    spine_count: usize,
    length: usize,
}

#[derive(Clone, Copy)]
struct RetainedApp {
    resume: ResumePoint,
    preferences: AppPreferences,
}

mod retained_resume {
    use defmt::info;
    use esp_hal::peripherals::LPWR;

    use super::{AppPreferences, DeviceLibrary, HomeItem, ResumePoint, RetainedApp};
    use crate::storage::book_resume::{RESUME_WORDS, SavedResume};

    #[esp_hal::ram(unstable(rtc_fast, persistent))]
    static mut RETAINED_RESUME: [u32; RESUME_WORDS] = [0; RESUME_WORDS];

    pub(super) struct RtcResume {
        low_power: LPWR<'static>,
    }

    impl RtcResume {
        pub(super) fn new(low_power: LPWR<'static>) -> Self {
            Self { low_power }
        }

        #[inline(never)]
        pub(super) fn read_resume(&mut self, library: &DeviceLibrary) -> Option<RetainedApp> {
            let words = unsafe { core::ptr::read_volatile(&raw const RETAINED_RESUME) };
            let saved = SavedResume::decode(&words).ok()?;
            let resume = saved
                .resolve(&library.files[..library.length])
                .unwrap_or_else(|error| {
                    info!("reader resume unavailable: {}", defmt::Debug2Format(&error));
                    ResumePoint::Home {
                        selected: HomeItem::Books,
                    }
                });
            Some(RetainedApp {
                resume,
                preferences: saved.preferences(),
            })
        }

        pub(super) fn write_resume(
            self,
            resume: ResumePoint,
            preferences: AppPreferences,
            library: &DeviceLibrary,
        ) -> LPWR<'static> {
            let words =
                match SavedResume::capture(resume, preferences, &library.files[..library.length]) {
                    Ok(saved) => saved.encode(),
                    Err(error) => {
                        info!("reader resume not saved: {}", defmt::Debug2Format(&error));
                        [0; RESUME_WORDS]
                    }
                };
            unsafe { core::ptr::write_volatile(&raw mut RETAINED_RESUME, words) };
            self.low_power
        }
    }
}

#[cfg(brewthink_previous_frame_storage = "host_ram")]
static DISPLAYED_FRAME: StaticCell<[u8; MONO_FRAME_BYTES]> = StaticCell::new();

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
    let mut rtc_resume = RtcResume::new(low_power);
    static STORE: StaticCell<DeviceStore> = StaticCell::new();
    static LIBRARY: ConstStaticCell<DeviceLibrary> = ConstStaticCell::new(DeviceLibrary::empty());
    static IMAGES: ConstStaticCell<DeviceImages> = ConstStaticCell::new(DeviceImages::empty());
    static ZIP: ConstStaticCell<ZipValidationScratch> =
        ConstStaticCell::new(ZipValidationScratch::new());
    static PACKAGE: ConstStaticCell<DevicePackageScratch> =
        ConstStaticCell::new(DevicePackageScratch::new());
    static NAVIGATION: ConstStaticCell<BookNavigation> =
        ConstStaticCell::new(BookNavigation::new());
    static FRAME_CODEC: ConstStaticCell<FrameCodecWorkspace> =
        ConstStaticCell::new(FrameCodecWorkspace::new());
    static CONTENT: ConstStaticCell<ContentWorkspace> =
        ConstStaticCell::new(ContentWorkspace::new());
    static RESOURCE: ConstStaticCell<[u8; MAX_DEVICE_RESOURCE_BYTES]> =
        ConstStaticCell::new([0; MAX_DEVICE_RESOURCE_BYTES]);
    static UPLOAD_BUFFER: ConstStaticCell<[u8; UPLOAD_CHUNK_BYTES]> =
        ConstStaticCell::new([0; UPLOAD_CHUNK_BYTES]);

    let mut card = Some(ReadOnlySdCard::new(hardware));
    let mut store: Option<&'static DeviceStore> = None;
    let mut workspaces = Workspaces {
        navigation: NAVIGATION.take(),
        zip: ZIP.take(),
        package: PACKAGE.take(),
        frame_codec: FRAME_CODEC.take(),
        content: CONTENT.take(),
        resource: RESOURCE.take(),
    };
    let library = LIBRARY.take();
    let images = IMAGES.take();
    let mut panel = None;
    let mut startup = Startup::Run(StartupStage::Card);
    let mut startup_commands = ControlLineBuffer::new();
    loop {
        match startup {
            Startup::Run(stage) => {
                let result = match stage {
                    StartupStage::Card => card
                        .as_mut()
                        .expect("card is owned until initialized")
                        .initialize()
                        .map(|_| ())
                        .map_err(ReaderError::Card),
                    StartupStage::Layout => store
                        .expect("card initialized")
                        .ensure_layout()
                        .map_err(ReaderError::File),
                    StartupStage::Books => {
                        load_library(store.expect("card initialized"), library, &mut workspaces)
                            .map_err(ReaderError::File)
                    }
                    StartupStage::Images => load_images(store.expect("card initialized"), images)
                        .map_err(ReaderError::AppData),
                    StartupStage::Display => {
                        let profile = X4DriveProfile::parse(X4_DRIVE_PROFILE)
                            .expect("build validates drive profile");
                        let mode = RefreshPolicyMode::parse(DISPLAY_REFRESH)
                            .expect("build validates refresh policy");
                        initialize_panel(store.expect("card initialized"), profile, mode)
                            .map(|display| panel = Some(display))
                            .map_err(ReaderError::Render)
                    }
                };
                if let Err(error) = startup.complete(result) {
                    esp_println::println!(
                        "BREWCTL/1 LOG stage=startup state=retry-required operation={:?} reason={}",
                        stage,
                        error
                    );
                } else if stage == StartupStage::Card {
                    store = Some(STORE.init_with(|| {
                        FatStorage::new(
                            X4FatBlockDevice::new(
                                card.take().expect("initialized card").enable_writes(),
                            ),
                            FixedTimeSource,
                        )
                    }));
                }
            }
            Startup::AwaitingRetry(stage) => {
                for _ in 0..64 {
                    let Ok(byte) = control.read_byte() else { break };
                    if let Some(command) = startup_commands.push(byte) {
                        match command {
                            Ok(ControlCommand::Tap(button)) => {
                                startup.input(map_button(button));
                                esp_println::println!("BREWCTL/1 DONE command=tap status=ok");
                            }
                            Ok(ControlCommand::Status) => {
                                esp_println::println!(
                                    "BREWCTL/1 STATUS view=startup operation={:?} retry=confirm",
                                    stage
                                );
                                esp_println::println!("BREWCTL/1 DONE command=status status=ok");
                            }
                            Ok(command) => {
                                let name = match command {
                                    ControlCommand::Screen => "screen",
                                    ControlCommand::Upload(_) => "upload",
                                    ControlCommand::AbortUpload => "abort-upload",
                                    ControlCommand::Tap(_) | ControlCommand::Status => {
                                        unreachable!()
                                    }
                                };
                                esp_println::println!(
                                    "BREWCTL/1 ERROR command={} reason=startup-recovery",
                                    name
                                );
                                esp_println::println!(
                                    "BREWCTL/1 DONE command={} status=error",
                                    name
                                );
                            }
                            Err(error) => {
                                esp_println::println!(
                                    "BREWCTL/1 ERROR command=parse reason={}",
                                    error.name()
                                );
                                esp_println::println!("BREWCTL/1 DONE command=parse status=error");
                            }
                        }
                    }
                }
                if let Either::First(event) = select(
                    INPUT_EVENTS.receive(),
                    Timer::after(Duration::from_millis(20)),
                )
                .await
                    && event.transition() == ButtonTransition::Pressed
                {
                    startup.input(map_button(event.button()));
                }
                if !matches!(startup, Startup::AwaitingRetry(_)) {
                    esp_println::println!(
                        "BREWCTL/1 LOG stage=startup state=continuing operation={:?} next={:?}",
                        stage,
                        startup
                    );
                }
            }
            Startup::Ready => break,
        }
    }
    let store = store.expect("startup initialized storage");
    let mut panel = panel.expect("startup initialized display");
    info!(
        "reader catalog ready: books={} images={} cached_spines={} path_bytes={}",
        library.length, images.length, library.spine_path_count, library.spine_path_byte_length
    );
    info!(
        "reader display configured: drive={=str} previous={=str} refresh={=str}",
        panel.display.drive_profile().name(),
        panel.display.previous_frame_storage().name(),
        panel.refresh_policy.mode().name()
    );
    let retained = rtc_resume.read_resume(library).unwrap_or(RetainedApp {
        resume: ResumePoint::Home {
            selected: HomeItem::Books,
        },
        preferences: AppPreferences::default(),
    });
    let preferences = match store.app_data().read_preferences() {
        Ok(Some(preferences)) => preferences,
        Ok(None) => retained.preferences,
        Err(error) => {
            info!(
                "reader preferences unavailable: {}",
                defmt::Display2Format(&error)
            );
            retained.preferences
        }
    };
    let selected_image = images.selected(store.app_data().read_selected_image());
    let (mut app, first_effect) = App::from_resume_with_catalog(
        library.length,
        images.length,
        selected_image.as_ref().copied().unwrap_or(None),
        preferences,
        retained.resume,
    )
    .unwrap_or_else(|error| {
        esp_println::println!("BREWCTL/1 LOG stage=resume state=home reason={:?}", error);
        (
            App::with_catalog(
                library.length,
                images.length,
                selected_image.as_ref().copied().unwrap_or(None),
                preferences,
            ),
            AppEffect::Render,
        )
    });
    if let Err(error) = images.apply_selection(&mut app, selected_image) {
        esp_println::println!(
            "BREWCTL/1 LOG stage=sleep-image state=unavailable reason={:?}",
            error
        );
    }
    if let Some(status) = BATTERY_STATUS.try_take() {
        let _ = app.set_battery(status);
    }
    let mut loaded = None;
    match run_effect(
        first_effect,
        &mut app,
        DeviceCatalogs { library, images },
        store,
        &mut panel,
        &mut workspaces,
        &mut loaded,
    ) {
        Ok(Some(resume)) => {
            enter_sleep(resume, app.preferences(), library, store, panel, rtc_resume).await;
        }
        Ok(None) => {}
        Err(status) => info!("{=str}", status),
    }
    if matches!(wakeup_cause, SleepSource::Gpio) {
        info!("reader resumed after GPIO3 deep-sleep wake");
    }

    let mut control_runtime = UsbControlRuntime::new(UPLOAD_BUFFER.take());
    esp_println::println!(
        "BREWCTL/1 READY version=1 width=480 height=800 bytes={}",
        FRAME_BYTES
    );
    write_control_status(&app);

    loop {
        if let Some(status) = BATTERY_STATUS.try_take() {
            let effect = app.set_battery(status);
            if control_runtime.is_upload_active() {
                continue;
            }
            match run_effect(
                effect,
                &mut app,
                DeviceCatalogs { library, images },
                store,
                &mut panel,
                &mut workspaces,
                &mut loaded,
            ) {
                Ok(Some(resume)) => {
                    enter_sleep(resume, app.preferences(), library, store, panel, rtc_resume).await;
                }
                Ok(None) => {}
                Err(status) => info!("{=str}", status),
            }
        }
        if let Some(event) =
            control_runtime.poll(&mut control, &app, workspaces.frame_codec.frame(), store)
        {
            let result = match event {
                ControlEvent::Button(button) => (ReaderRuntime {
                    app: &mut app,
                    library,
                    images,
                    store,
                    panel: &mut panel,
                    workspaces: &mut workspaces,
                    loaded: &mut loaded,
                })
                .run_input(InputSource::Usb, button),
                ControlEvent::ImagesChanged => match load_images(store, images) {
                    Ok(()) => {
                        let selected = images.selected(store.app_data().read_selected_image());
                        let effect = app.replace_image_catalog(
                            images.length,
                            selected.as_ref().copied().unwrap_or(None),
                        );
                        if let Err(error) = images.apply_selection(&mut app, selected) {
                            esp_println::println!(
                                "BREWCTL/1 LOG stage=sleep-image state=unavailable reason={:?}",
                                error
                            );
                        }
                        run_effect(
                            effect,
                            &mut app,
                            DeviceCatalogs { library, images },
                            store,
                            &mut panel,
                            &mut workspaces,
                            &mut loaded,
                        )
                    }
                    Err(error) => {
                        esp_println::println!(
                            "BREWCTL/1 LOG stage=image-scan state=retained reason={:?}",
                            error
                        );
                        Err("reader image scan failed; catalog retained")
                    }
                },
            };
            match result {
                Ok(Some(resume)) => {
                    enter_sleep(resume, app.preferences(), library, store, panel, rtc_resume).await;
                }
                Ok(None) => {}
                Err(status) => info!("{=str}", status),
            }
        }

        let next_control_poll = Timer::after(Duration::from_millis(20));
        let Either::First(event) = select(INPUT_EVENTS.receive(), next_control_poll).await else {
            continue;
        };
        if event.transition() != ButtonTransition::Pressed || control_runtime.is_upload_active() {
            continue;
        }

        match (ReaderRuntime {
            app: &mut app,
            library,
            images,
            store,
            panel: &mut panel,
            workspaces: &mut workspaces,
            loaded: &mut loaded,
        })
        .run_input(InputSource::Physical, event.button())
        {
            Ok(Some(resume)) => {
                enter_sleep(resume, app.preferences(), library, store, panel, rtc_resume).await;
            }
            Ok(None) => {}
            Err(status) => info!("{=str}", status),
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
    images: &'a DeviceImages,
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
        let previous_preferences = self.app.preferences();
        let previous_image = self.app.selected_sleep_image();
        let result = run_effect(
            self.app.input(map_button(button)),
            self.app,
            DeviceCatalogs {
                library: self.library,
                images: self.images,
            },
            self.store,
            self.panel,
            self.workspaces,
            self.loaded,
        );

        let result = reader_orchestration::persist_input_preferences(
            result,
            previous_preferences,
            self.app,
            |preferences| {
                if self
                    .store
                    .app_data()
                    .write_preferences(preferences)
                    .is_err()
                {
                    esp_println::println!(
                        "BREWCTL/1 LOG stage=preferences state=volatile reason=app-data-unavailable"
                    );
                } else {
                    esp_println::println!("BREWCTL/1 LOG stage=preferences state=persisted");
                }
            },
        );

        match result {
            Ok(resume) => {
                if self.app.selected_sleep_image() != previous_image
                    && let Some(image) = self.app.selected_sleep_image()
                    && let Some(file) = self.images.file(image)
                {
                    if self
                        .store
                        .app_data()
                        .write_selected_image(*file.name())
                        .is_err()
                    {
                        esp_println::println!(
                            "BREWCTL/1 LOG stage=sleep-image state=volatile reason=app-data-unavailable"
                        );
                    } else {
                        esp_println::println!("BREWCTL/1 LOG stage=sleep-image state=persisted");
                    }
                }
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

enum ControlEvent {
    Button(Button),
    ImagesChanged,
}

struct UsbControlRuntime<'a> {
    lines: ControlLineBuffer,
    transfer: FileTransfer,
    upload_buffer: &'a mut [u8; UPLOAD_CHUNK_BYTES],
    upload_buffer_length: usize,
    idle_polls: usize,
}

impl<'a> UsbControlRuntime<'a> {
    fn new(upload_buffer: &'a mut [u8; UPLOAD_CHUNK_BYTES]) -> Self {
        Self {
            lines: ControlLineBuffer::new(),
            transfer: FileTransfer::new(),
            upload_buffer,
            upload_buffer_length: 0,
            idle_polls: 0,
        }
    }

    fn is_upload_active(&self) -> bool {
        self.transfer.is_active()
    }

    fn poll(
        &mut self,
        control: &mut UsbSerialJtagRx<'static, Blocking>,
        app: &App,
        frame: &[u8; FRAME_BYTES],
        store: &DeviceStore,
    ) -> Option<ControlEvent> {
        if self.transfer.is_active() {
            self.idle_polls = self.idle_polls.saturating_add(1);
            if self.idle_polls >= UPLOAD_IDLE_POLLS {
                self.abort_upload(store, "timeout");
            }
        }

        while let Ok(byte) = control.read_byte() {
            self.idle_polls = 0;
            if self.transfer.is_active() {
                self.upload_buffer[self.upload_buffer_length] = byte;
                self.upload_buffer_length += 1;
                let request = self
                    .transfer
                    .request()
                    .expect("active upload has a request");
                let remaining = request.length() - self.transfer.received();
                if self.upload_buffer_length == remaining.min(UPLOAD_CHUNK_BYTES)
                    && self.flush_upload_chunk(store)
                {
                    return Some(ControlEvent::ImagesChanged);
                }
                continue;
            }

            let Some(parsed) = self.lines.push(byte) else {
                continue;
            };
            match parsed {
                Ok(ControlCommand::Tap(button)) => return Some(ControlEvent::Button(button)),
                Ok(ControlCommand::Status) => {
                    write_control_status(app);
                    esp_println::println!("BREWCTL/1 DONE command=status status=ok");
                }
                Ok(ControlCommand::Screen) => {
                    write_control_screen(frame);
                    esp_println::println!("BREWCTL/1 DONE command=screen status=ok");
                }
                Ok(ControlCommand::Upload(request)) => self.begin_upload(request, store),
                Ok(ControlCommand::AbortUpload) => self.abort_upload(store, "requested"),
                Err(error) => {
                    esp_println::println!("BREWCTL/1 ERROR command=parse reason={}", error.name());
                    esp_println::println!("BREWCTL/1 DONE command=parse status=error");
                }
            }
        }
        None
    }

    fn begin_upload(&mut self, request: UploadRequest, store: &DeviceStore) {
        let mut app_data = store.app_data();
        match self.transfer.begin(request, &mut app_data) {
            Ok(()) => {
                self.upload_buffer_length = 0;
                esp_println::println!(
                    "BREWCTL/1 READY command=upload chunk={} bytes={}",
                    UPLOAD_CHUNK_BYTES,
                    request.length()
                );
            }
            Err(error) => {
                esp_println::println!("BREWCTL/1 ERROR command=upload reason={}", error.name());
                esp_println::println!("BREWCTL/1 DONE command=upload status=error");
            }
        }
    }

    fn flush_upload_chunk(&mut self, store: &DeviceStore) -> bool {
        let length = self.upload_buffer_length;
        let mut app_data = store.app_data();
        if let Err(error) = self
            .transfer
            .append(&self.upload_buffer[..length], &mut app_data)
        {
            let _ = self.transfer.abort(&mut app_data);
            self.upload_buffer_length = 0;
            esp_println::println!("BREWCTL/1 ERROR command=upload reason={}", error.name());
            esp_println::println!("BREWCTL/1 DONE command=upload status=error");
            return false;
        }
        self.upload_buffer_length = 0;
        let request = self
            .transfer
            .request()
            .expect("active upload has a request");
        esp_println::println!(
            "BREWCTL/1 ACK command=upload received={} total={}",
            self.transfer.received(),
            request.length()
        );
        if self.transfer.received() != request.length() {
            return false;
        }
        match self.transfer.finish(&mut app_data, self.upload_buffer) {
            Ok(_) => {
                esp_println::println!("BREWCTL/1 DONE command=upload status=ok");
                true
            }
            Err(error) => {
                esp_println::println!("BREWCTL/1 ERROR command=upload reason={}", error.name());
                esp_println::println!("BREWCTL/1 DONE command=upload status=error");
                false
            }
        }
    }

    fn abort_upload(&mut self, store: &DeviceStore, reason: &str) {
        let mut app_data = store.app_data();
        let was_active = self.transfer.is_active();
        let result = self.transfer.abort(&mut app_data);
        self.upload_buffer_length = 0;
        self.idle_polls = 0;
        if was_active || result.is_err() {
            esp_println::println!("BREWCTL/1 ERROR command=upload reason={}", reason);
            esp_println::println!("BREWCTL/1 DONE command=upload status=error");
        } else {
            esp_println::println!("BREWCTL/1 DONE command=upload-abort status=ok");
        }
    }
}

fn write_control_status(app: &App) {
    match app.view() {
        AppView::Home(state) => {
            esp_println::println!(
                "BREWCTL/1 STATUS view=home selected={}",
                state.selected().index()
            );
        }
        AppView::BookCover { book, .. } => {
            esp_println::println!("BREWCTL/1 STATUS view=cover book={}", book.index())
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
                "BREWCTL/1 STATUS view=files selected={} files={} images={}",
                selected.index(),
                state.file_count(),
                app.image_count()
            ),
            None => esp_println::println!(
                "BREWCTL/1 STATUS view=files selected=none files={} images={}",
                state.file_count(),
                app.image_count()
            ),
        },
        AppView::Settings(state) => {
            esp_println::println!(
                "BREWCTL/1 STATUS view=settings selected={} preferences={}",
                state.selected().index(),
                state.draft().packed()
            );
        }
        AppView::Loading(_) => {
            esp_println::println!("BREWCTL/1 STATUS view=loading");
        }
        AppView::ReaderDrawer(drawer) => {
            let location = drawer.session().location();
            esp_println::println!(
                "BREWCTL/1 STATUS view=reader-drawer book={} spine={} page={} pages={}",
                location.book().index(),
                location.spine_index(),
                location.page_index(),
                location.page_count()
            );
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
        AppView::Image(image) => {
            esp_println::println!(
                "BREWCTL/1 STATUS view=image image={} selected={}",
                image.index(),
                app.selected_sleep_image() == Some(image)
            );
        }
        AppView::Error { book, origin: _ } => {
            esp_println::println!("BREWCTL/1 STATUS view=error book={}", book.index());
        }
        AppView::Sleeping { .. } => {
            esp_println::println!("BREWCTL/1 STATUS view=sleeping");
        }
    }
    write_control_battery_status(app.battery());
}

fn write_control_battery_status(battery: BatteryStatus) {
    match (battery.level(), battery.voltage()) {
        (BatteryLevel::Unknown, None) => esp_println::println!(
            "BREWCTL/1 BATTERY percent=unknown millivolts=unknown usb={}",
            battery.usb().name()
        ),
        (BatteryLevel::Unknown, Some(voltage)) => esp_println::println!(
            "BREWCTL/1 BATTERY percent=unknown millivolts={} usb={}",
            voltage.millivolts().get(),
            battery.usb().name()
        ),
        (BatteryLevel::Percent(percent), None) => esp_println::println!(
            "BREWCTL/1 BATTERY percent={} millivolts=unknown usb={}",
            percent.get(),
            battery.usb().name()
        ),
        (BatteryLevel::Percent(percent), Some(voltage)) => esp_println::println!(
            "BREWCTL/1 BATTERY percent={} millivolts={} usb={}",
            percent.get(),
            voltage.millivolts().get(),
            battery.usb().name()
        ),
    }
}

fn write_control_screen(frame: &[u8; FRAME_BYTES]) {
    let crc32 = crc32fast::hash(frame);
    esp_println::println!(
        "BREWCTL/1 SCREEN width=480 height=800 bytes={} crc32={:08x} bpp={} encoding=planar",
        frame.len(),
        crc32,
        READER_DEPTH.bits(),
    );
    esp_println::Printer::write_bytes(frame);
    esp_println::Printer::write_bytes(b"\n");
}

#[inline(never)]
fn load_library(
    store: &DeviceStore,
    library: &mut DeviceLibrary,
    workspaces: &mut Workspaces,
) -> Result<(), FileError> {
    info!("reader startup: book directory scan start");
    let catalog = workspaces.content.prepare_catalog();
    store.scan_into(catalog)?;
    library.length = 0;
    library.spine_path_count = 0;
    library.spine_path_byte_length = 0;
    info!(
        "reader startup: book directory scan done entries={}",
        catalog.len()
    );
    for (catalog_index, file) in catalog.books().copied().enumerate() {
        info!(
            "reader startup: book validation start index={} bytes={}",
            catalog_index,
            file.size()
        );
        let index = library.length;
        let book_id = BookId::new(index);
        library.files[index] = Some(file);
        library.titles[index] = FixedString::try_from_str("Unavailable EPUB").expect("title fits");
        library.creators[index] = FixedString::new();
        library.cover_paths[index] = None;
        library.spine_counts[index] = 0;
        library.book_cached_spine_counts[index] = 0;
        library.length += 1;
        let (inflate, publication) = workspaces.frame_codec.prepare_epub();
        let reader = match store.open_reader(file) {
            Ok(reader) => reader,
            Err(error) => {
                esp_println::println!(
                    "BREWCTL/1 LOG stage=book-open state=unavailable book={} reason={:?}",
                    index,
                    error
                );
                continue;
            }
        };
        info!("reader startup: book open done index={}", catalog_index);
        let book = match DeviceEpub::open(
            reader,
            workspaces.zip,
            workspaces.package,
            inflate,
            workspaces.resource,
            publication,
        ) {
            Ok(book) => book,
            Err(error) => {
                esp_println::println!(
                    "BREWCTL/1 LOG stage=book-validation state=unavailable book={} reason={:?}",
                    index,
                    error
                );
                continue;
            }
        };
        info!(
            "reader startup: book validation done index={}",
            catalog_index
        );
        let publication = book.publication();
        let title = FixedString::try_from_str(publication.title())
            .expect("publication and catalog title capacities match");
        let creator = FixedString::try_from_str(publication.creator())
            .expect("publication and catalog creator capacities match");
        let cover_path = publication.cover_path().map(|path| {
            FixedString::try_from_str(path).expect("publication and catalog path capacities match")
        });
        let spine_count = u8::try_from(publication.spine_len())
            .expect("bounded publication has at most 64 spine items");
        library.cache_spine_paths(book_id, publication);
        library.files[index] = Some(file);
        library.titles[index] = title;
        library.creators[index] = creator;
        library.cover_paths[index] = cover_path;
        library.spine_counts[index] = spine_count;
    }
    Ok(())
}

fn load_images(
    store: &DeviceStore,
    images: &mut DeviceImages,
) -> Result<(), crate::storage::AppDataError<crate::x4::X4FatBlockDeviceError>> {
    images.scanned(store.app_data().scan_images::<MAX_DEVICE_IMAGES>())
}

fn initialize_panel(
    store: &DeviceStore,
    profile: X4DriveProfile,
    refresh_policy_mode: RefreshPolicyMode,
) -> Result<ReaderDisplay, &'static str> {
    store.with_device(|device| {
        device.with_hardware(|hardware| {
            let mut bus = hardware.display_bus().map_err(|error| {
                esp_println::println!(
                    "BREWCTL/1 LOG stage=display-session state=failed reason={:?}",
                    error
                );
                "reader display session failed"
            })?;
            let controller = Ssd1677::with_profile(profile)
                .initialize(&mut bus)
                .map_err(|error| {
                    esp_println::println!(
                        "BREWCTL/1 LOG stage=display-init state=failed reason={:?}",
                        error
                    );
                    "reader display initialization failed"
                })?;
            #[cfg(brewthink_previous_frame_storage = "host_ram")]
            let display = BufferedDisplay::with_host_ram(
                controller,
                DISPLAYED_FRAME.init_with(|| [0xFF; MONO_FRAME_BYTES]),
                Rotation::Degrees270,
            );
            #[cfg(brewthink_previous_frame_storage = "controller_ram")]
            let display = BufferedDisplay::with_controller_ram(controller, Rotation::Degrees270);
            Ok(ReaderDisplay {
                display,
                refresh_policy: RefreshPolicy::new(refresh_policy_mode),
            })
        })
    })
}

fn run_effect(
    effect: AppEffect,
    app: &mut App,
    catalogs: DeviceCatalogs<'_>,
    store: &DeviceStore,
    panel: &mut ReaderDisplay,
    workspaces: &mut Workspaces,
    loaded: &mut Option<LoadedChapter>,
) -> Result<Option<ResumePoint>, &'static str> {
    reader_orchestration::run_effect(
        effect,
        app,
        &mut DeviceReaderIo {
            catalogs,
            store,
            panel,
            workspaces,
            loaded,
        },
    )
    .map_err(|failure| {
        esp_println::println!(
            "BREWCTL/1 LOG stage=reader-operation state={} reason={:?}",
            if failure.recovery.is_none() {
                "recovered"
            } else {
                "recovery-failed"
            },
            failure
        );
        "reader operation failed; input remains available"
    })
}

struct DeviceReaderIo<'a> {
    catalogs: DeviceCatalogs<'a>,
    store: &'a DeviceStore,
    panel: &'a mut ReaderDisplay,
    workspaces: &'a mut Workspaces,
    loaded: &'a mut Option<LoadedChapter>,
}

impl ReaderIo for DeviceReaderIo<'_> {
    type Error = ReaderError;

    fn load_chapter(
        &mut self,
        book: BookId,
        spine_index: usize,
        preferences: crate::app::ReaderPreferences,
    ) -> Result<ChapterPages, Self::Error> {
        *self.loaded = None;
        esp_println::println!(
            "BREWCTL/1 LOG stage=load-chapter state=start book={} spine={}",
            book.index(),
            spine_index
        );
        let chapter = load_chapter(
            book,
            spine_index,
            self.catalogs.library,
            self.store,
            self.workspaces,
        )?;
        let page = self.workspaces.content.prepare_page();
        layout_xhtml_page_into(
            &self.workspaces.resource[..chapter.length],
            0,
            preferences,
            page,
        )
        .map_err(ReaderError::Layout)?;
        let pages = ChapterPages {
            spine_count: chapter.spine_count,
            page_count: page.page_count(),
        };
        *self.loaded = Some(chapter);
        esp_println::println!(
            "BREWCTL/1 LOG stage=load-chapter state=done book={} spine={}",
            book.index(),
            spine_index
        );
        Ok(pages)
    }

    fn render(&mut self, app: &App) -> Result<Rendered, Self::Error> {
        let library = self.catalogs.library;
        let images = self.catalogs.images;
        let store = self.store;
        let workspaces = &mut *self.workspaces;
        let result = (|| {
            match app.view() {
                AppView::BookCover { book, .. } => {
                    return reader_orchestration::render_cover(
                        decode_book_cover_frame(book, library, store, workspaces),
                        |error| {
                            esp_println::println!(
                                "BREWCTL/1 LOG stage=cover state=unavailable book={} reason={}",
                                book.index(),
                                error
                            );
                        },
                        || refresh(store, self.panel, workspaces.frame_codec.frame()),
                    );
                }
                AppView::Home(_) => render_home_frame(app, workspaces.frame_codec.frame())?,
                AppView::Library => render_library(app, library, store, workspaces)?,
                AppView::Files(_) => {
                    render_files_frame(app, library, images, workspaces.frame_codec.frame())?
                }
                AppView::Settings(_) => render_settings_frame(app, images, store, workspaces)?,
                AppView::Loading(_) => return Err("reader render requested while loading"),
                AppView::Reader(_) | AppView::ReaderDrawer(_) => {
                    let location = match app.view() {
                        AppView::Reader(session) => session.location(),
                        AppView::ReaderDrawer(drawer) => drawer.session().location(),
                        _ => unreachable!(),
                    };
                    let chapter = self.loaded.ok_or("reader chapter was not loaded")?;
                    if chapter.book != location.book()
                        || chapter.spine_index != location.spine_index()
                    {
                        return Err("reader chapter cache mismatch");
                    }
                    render_page(
                        app,
                        location,
                        library,
                        &workspaces.resource[..chapter.length],
                        workspaces.content.prepare_page(),
                        &workspaces.navigation.titles,
                        workspaces.frame_codec.frame(),
                    )?;
                }
                AppView::Image(image) => render_image_frame(app, image, images, store, workspaces)?,
                AppView::Error { book, .. } => {
                    render_error(app, book, library, workspaces.frame_codec.frame())?
                }
                AppView::Sleeping { resume } => {
                    render_sleep_frame(app, resume, library, images, store, workspaces)?
                }
            }
            refresh(store, self.panel, workspaces.frame_codec.frame())?;
            Ok(Rendered::Frame)
        })();
        result.map_err(ReaderError::Render)
    }
}

fn load_chapter(
    selected: BookId,
    spine_index: usize,
    library: &DeviceLibrary,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<LoadedChapter, ReaderError> {
    let file = library.file(selected).ok_or(ReaderError::BookMissing)?;
    let (inflate, publication) = workspaces.frame_codec.prepare_epub();
    let reader = store.open_reader(file).map_err(ReaderError::File)?;
    let (spine_count, length) = if let Some(path) = library
        .spine_path(selected, spine_index)
        .filter(|_| workspaces.navigation.book == Some(selected))
    {
        let archive = StreamingZip::open(reader, workspaces.zip).map_err(ReaderError::Archive)?;
        let entry = archive.find(path).map_err(ReaderError::Archive)?;
        if entry.uncompressed_size() as usize > workspaces.resource.len() {
            return Err(ReaderError::ResourceTooLarge);
        }
        let length = archive
            .read_entry(entry, workspaces.resource, inflate)
            .map_err(ReaderError::Archive)?;
        (library.spine_count(selected), length)
    } else {
        let book = DeviceEpub::open(
            reader,
            workspaces.zip,
            workspaces.package,
            inflate,
            workspaces.resource,
            publication,
        )
        .map_err(ReaderError::Epub)?;
        let spine_count = book.publication().spine_len();
        if workspaces.navigation.book != Some(selected) {
            if let Err(error) = book.read_chapter_titles(
                &mut workspaces.navigation.titles,
                workspaces.resource,
                inflate,
            ) {
                esp_println::println!(
                    "BREWCTL/1 LOG stage=chapter-navigation state=fallback book={} reason={:?}",
                    selected.index(),
                    error
                );
            }
            workspaces.navigation.book = Some(selected);
        }
        let length = book
            .read_spine(spine_index, workspaces.resource, inflate)
            .map_err(ReaderError::Epub)?;
        (spine_count, length)
    };
    Ok(LoadedChapter {
        book: selected,
        spine_index,
        spine_count,
        length,
    })
}

fn read_book_cover(
    selected: BookId,
    library: &DeviceLibrary,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
    maximum: usize,
) -> Result<Option<usize>, &'static str> {
    let Some(path) = library.cover_path(selected) else {
        return Ok(None);
    };
    esp_println::println!(
        "BREWCTL/1 LOG stage=cover state=start book={}",
        selected.index()
    );
    let file = library.file(selected).ok_or("reader book is missing")?;
    let inflate = workspaces.frame_codec.prepare_inflate();
    let reader = store.open_reader(file).map_err(|error| {
        esp_println::println!(
            "BREWCTL/1 LOG stage=cover-open state=failed reason={:?}",
            error
        );
        "reader cover file open failed"
    })?;
    let archive = StreamingZip::open(reader, workspaces.zip).map_err(|error| {
        esp_println::println!(
            "BREWCTL/1 LOG stage=cover-archive state=failed reason={:?}",
            error
        );
        "reader cover archive open failed"
    })?;
    esp_println::println!(
        "BREWCTL/1 LOG stage=cover state=archive-open book={}",
        selected.index()
    );
    let entry = archive.find(path).map_err(|error| {
        esp_println::println!(
            "BREWCTL/1 LOG stage=cover-entry state=failed reason={:?}",
            error
        );
        "reader cover entry is unavailable"
    })?;
    esp_println::println!(
        "BREWCTL/1 LOG stage=cover state=entry-found book={} compressed={} uncompressed={}",
        selected.index(),
        entry.compressed_size(),
        entry.uncompressed_size()
    );
    if !encoded_cover_fits(entry.compressed_size(), entry.uncompressed_size())
        || entry.uncompressed_size() as usize > maximum
    {
        esp_println::println!(
            "BREWCTL/1 LOG stage=cover state=skipped book={} reason=encoded-size",
            selected.index()
        );
        return Ok(None);
    }
    let maximum = maximum.min(MAX_ENCODED_COVER_BYTES as usize);
    let length = archive
        .read_entry(entry, &mut workspaces.resource[..maximum], inflate)
        .map_err(|error| {
            esp_println::println!(
                "BREWCTL/1 LOG stage=cover-read state=failed reason={:?}",
                error
            );
            "reader cover read failed"
        })?;
    Ok(Some(length))
}

fn decode_book_cover(
    selected: BookId,
    library: &DeviceLibrary,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<bool, &'static str> {
    let Some(length) = read_book_cover(
        selected,
        library,
        store,
        workspaces,
        MAX_DEVICE_RESOURCE_BYTES,
    )?
    else {
        return Ok(false);
    };
    let encoded = &workspaces.resource[..length];
    let output = &mut *workspaces.content.cover();
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
    decoded.map_err(|error| {
        esp_println::println!(
            "BREWCTL/1 LOG stage=cover-decode state=failed reason={:?}",
            error
        );
        "reader cover decode failed"
    })?;
    esp_println::println!(
        "BREWCTL/1 LOG stage=cover state=done book={}",
        selected.index()
    );
    Ok(true)
}

fn decode_book_cover_frame(
    selected: BookId,
    library: &DeviceLibrary,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<bool, &'static str> {
    let Some(length) =
        read_book_cover(selected, library, store, workspaces, MAX_DEVICE_IMAGE_BYTES)?
    else {
        return Ok(false);
    };
    let Some(format) = ImageFormat::detect(&workspaces.resource[..length]) else {
        return Ok(false);
    };
    decode_resource_frame(length, format, ScaleMode::Contain, workspaces)?;
    Ok(true)
}

fn decode_image_preview(
    file: &ImageFile,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), &'static str> {
    let loaded = store
        .app_data()
        .read_image(
            *file.name(),
            &mut workspaces.resource[..MAX_DEVICE_IMAGE_BYTES],
        )
        .map_err(|error| {
            esp_println::println!(
                "BREWCTL/1 LOG stage=image-preview-read state=failed reason={:?}",
                error
            );
            "reader image preview read failed"
        })?;
    let encoded = &workspaces.resource[..loaded.length()];
    let output = &mut *workspaces.content.cover();
    let decoded = match loaded.format() {
        ImageFormat::Jpeg => workspaces
            .frame_codec
            .with_jpeg(|workspace| decode_jpeg_cover(encoded, output, workspace)),
        ImageFormat::Png => workspaces
            .frame_codec
            .with_png(|workspace| decode_png_cover(encoded, output, workspace)),
    };
    decoded.map(|_| ()).map_err(|error| {
        esp_println::println!(
            "BREWCTL/1 LOG stage=image-preview-decode state=failed reason={:?}",
            error
        );
        "reader image preview decode failed"
    })
}

fn decode_image_frame(
    file: &ImageFile,
    scale: ScaleMode,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), &'static str> {
    let loaded = store
        .app_data()
        .read_image(
            *file.name(),
            &mut workspaces.resource[..MAX_DEVICE_IMAGE_BYTES],
        )
        .map_err(|error| {
            esp_println::println!(
                "BREWCTL/1 LOG stage=image-read state=failed reason={:?}",
                error
            );
            "reader image read failed"
        })?;
    decode_resource_frame(loaded.length(), loaded.format(), scale, workspaces)
}

fn decode_resource_frame(
    length: usize,
    format: ImageFormat,
    scale: ScaleMode,
    workspaces: &mut Workspaces,
) -> Result<(), &'static str> {
    let (encoded_buffer, decoder_buffer) = workspaces.resource.split_at_mut(MAX_DEVICE_IMAGE_BYTES);
    let encoded = &encoded_buffer[..length];
    let frame = workspaces.frame_codec.frame();
    let mut target = PackedImage::new(frame_size(), READER_DEPTH, frame)
        .map_err(|_| "reader frame buffer has the wrong size")?;
    let options = RenderOptions {
        scale,
        dither: Dither::None,
    };
    match format {
        ImageFormat::Jpeg => decode_jpeg(
            encoded,
            &mut target,
            options,
            JpegDecodeWorkspace::in_buffer(decoder_buffer)
                .ok_or("reader JPEG workspace does not fit")?,
        ),
        ImageFormat::Png => decode_png(
            encoded,
            &mut target,
            options,
            CoverDecodeWorkspace::in_buffer(decoder_buffer)
                .ok_or("reader PNG workspace does not fit")?,
        ),
    }
    .map_err(|error| {
        esp_println::println!(
            "BREWCTL/1 LOG stage=image-decode state=failed reason={:?}",
            error
        );
        "reader image decode failed"
    })?;
    Ok(())
}

fn render_home_frame(app: &App, frame: &mut [u8; FRAME_BYTES]) -> Result<(), &'static str> {
    let mut image = PackedImage::new(frame_size(), READER_DEPTH, frame)
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_app(
        AppFrame::Home {
            state: app.home(),
            battery: app.battery(),
        },
        &mut image,
    )
    .map_err(|_| "reader home render failed")
}

fn render_files_frame(
    app: &App,
    library: &DeviceLibrary,
    images: &DeviceImages,
    frame: &mut [u8; FRAME_BYTES],
) -> Result<(), &'static str> {
    let mut files = [FileItem::new("", 0, FileKind::Epub); MAX_DEVICE_FILES];
    for (index, file) in files[..library.length].iter_mut().enumerate() {
        let book = BookId::new(index);
        *file = FileItem::new(
            library.file_name(book),
            library.file_size(book),
            FileKind::Epub,
        );
    }
    for (index, file) in files[library.length..library.length + images.length]
        .iter_mut()
        .enumerate()
    {
        let image = images
            .file(ImageId::new(index))
            .ok_or("image catalog mismatch")?;
        let kind = match image.format() {
            ImageFormat::Jpeg => FileKind::Jpeg,
            ImageFormat::Png => FileKind::Png,
        };
        *file = FileItem::new(image.name().as_str(), image.size(), kind);
    }
    let mut image = PackedImage::new(frame_size(), READER_DEPTH, frame)
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_app(
        AppFrame::Files {
            state: app.files(),
            files: &files[..library.length + images.length],
            battery: app.battery(),
        },
        &mut image,
    )
    .map_err(|_| "reader files render failed")
}

fn render_settings_frame(
    app: &App,
    images: &DeviceImages,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), &'static str> {
    let AppView::Settings(settings) = app.view() else {
        return Err("reader settings render requested outside settings");
    };
    let show_custom_preview = settings.selected() == SettingsItem::SleepScreen
        && settings.draft().sleep_screen() != SleepScreenMode::BookCover;
    let selected_file = app
        .selected_sleep_image()
        .filter(|_| show_custom_preview)
        .and_then(|image| images.file(image));
    let custom_image = match selected_file {
        Some(file) => match decode_image_preview(file, store, workspaces) {
            Ok(()) => CustomImagePreview::Ready {
                name: file.name().as_str(),
                bitmap: bitmap(workspaces.content.cover()),
            },
            Err(error) => {
                esp_println::println!(
                    "BREWCTL/1 LOG stage=image-preview state=unavailable reason={}",
                    error
                );
                CustomImagePreview::Invalid
            }
        },
        None => CustomImagePreview::Missing,
    };
    let mut image = PackedImage::new(frame_size(), READER_DEPTH, workspaces.frame_codec.frame())
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_app(
        AppFrame::Settings {
            state: settings,
            battery: app.battery(),
            custom_image,
        },
        &mut image,
    )
    .map_err(|_| "reader settings render failed")
}

fn render_image_frame(
    app: &App,
    image: ImageId,
    images: &DeviceImages,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), &'static str> {
    let file = images
        .file(image)
        .ok_or("image selection is out of bounds")?;
    decode_image_frame(file, ScaleMode::Contain, store, workspaces)?;
    let mut target = PackedImage::new(frame_size(), READER_DEPTH, workspaces.frame_codec.frame())
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_image_viewer(
        file.name().as_str(),
        app.selected_sleep_image() == Some(image),
        app.battery(),
        &mut target,
    )
    .map_err(|_| "reader image viewer render failed")
}

fn render_library(
    app: &App,
    library: &DeviceLibrary,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), &'static str> {
    let visible = app.library().visible_range();
    let selected = app.library().selected().map(BookId::index);
    let selected_slot = selected.map_or(0, |index| index - visible.start);
    let mut decoded = [false; VISIBLE_COVER_SLOTS];
    for (slot, index) in visible.clone().enumerate() {
        if Some(index) == selected {
            continue;
        }
        decoded[slot] = decode_book_cover(BookId::new(index), library, store, workspaces)
            .unwrap_or_else(|error| {
                esp_println::println!(
                    "BREWCTL/1 LOG stage=shelf-cover state=unavailable book={} reason={}",
                    index,
                    error
                );
                false
            });
        if decoded[slot] {
            let cache_slot = slot - usize::from(selected_slot < slot);
            let offset = MAX_ENCODED_COVER_BYTES as usize + cache_slot * SHELF_COVER_BYTES;
            downsample_cover(
                workspaces.content.cover(),
                &mut workspaces.resource[offset..offset + SHELF_COVER_BYTES],
            );
        }
    }
    if let Some(index) = selected.filter(|index| visible.contains(index)) {
        decoded[index - visible.start] =
            decode_book_cover(BookId::new(index), library, store, workspaces).unwrap_or_else(
                |error| {
                    esp_println::println!(
                        "BREWCTL/1 LOG stage=shelf-cover state=unavailable book={} reason={}",
                        index,
                        error
                    );
                    false
                },
            );
    }
    let covers = &workspaces.resource[MAX_ENCODED_COVER_BYTES as usize..];
    let full_cover = &*workspaces.content.cover();
    let mut books = [ShelfBook::new("", "", None); MAX_DEVICE_BOOKS];
    for (index, book) in books[..library.length].iter_mut().enumerate() {
        let cover = visible
            .contains(&index)
            .then(|| index - visible.start)
            .filter(|&slot| decoded[slot])
            .map(|slot| {
                if Some(index) == selected {
                    bitmap(full_cover)
                } else {
                    let cache_slot = slot - usize::from(selected_slot < slot);
                    let offset = cache_slot * SHELF_COVER_BYTES;
                    shelf_bitmap(&covers[offset..offset + SHELF_COVER_BYTES])
                }
            });
        *book = ShelfBook::new(
            library.titles[index].as_str(),
            library.creators[index].as_str(),
            cover,
        );
    }
    let mut image = PackedImage::new(frame_size(), READER_DEPTH, workspaces.frame_codec.frame())
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_app(
        AppFrame::Library {
            state: app.library(),
            books: &books[..library.length],
            battery: app.battery(),
        },
        &mut image,
    )
    .map_err(|_| "reader shelf render failed")
}

fn render_page(
    app: &App,
    location: ReadingLocation,
    library: &DeviceLibrary,
    xhtml: &[u8],
    page: &mut BoundedPage,
    chapter_titles: &[FixedString<{ crate::navigation::CHAPTER_TITLE_BYTES }>],
    frame: &mut [u8; FRAME_BYTES],
) -> Result<(), &'static str> {
    layout_xhtml_page_into(xhtml, location.page_index(), app.reader_preferences(), page).map_err(
        |error| {
            esp_println::println!(
                "BREWCTL/1 LOG stage=page-layout state=failed reason={:?}",
                error
            );
            "reader requested page layout failed"
        },
    )?;
    let mut lines = [ReaderLine::new("", ReaderStyle::Body); MAX_PAGE_LINES];
    let mut line_count = 0;
    for line in page.lines() {
        lines[line_count] = ReaderLine::new(line.text(), line.style());
        line_count += 1;
    }
    let chapter_index = match app.view() {
        AppView::ReaderDrawer(drawer) => drawer.chapter(),
        _ => location.spine_index(),
    };
    let mut fallback = FixedString::<64>::new();
    write!(fallback, "Chapter {}", chapter_index + 1).ok();
    let chapter_title = chapter_titles
        .get(chapter_index)
        .filter(|title| !title.is_empty())
        .map_or(fallback.as_str(), FixedString::as_str);
    let mut view = ReaderView::new(
        library.title(location.book()),
        chapter_title,
        &lines[..line_count],
        app.reader_preferences(),
        app.battery(),
    );
    if let AppView::ReaderDrawer(drawer) = app.view() {
        view = view.with_drawer(drawer);
    }
    let mut image = PackedImage::new(frame_size(), READER_DEPTH, frame)
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_app(AppFrame::Reader(view), &mut image).map_err(|_| "reader page render failed")
}

fn render_sleep_frame(
    app: &App,
    resume: ResumePoint,
    library: &DeviceLibrary,
    images: &DeviceImages,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), &'static str> {
    let mut status = FixedString::<64>::new();
    match resume {
        ResumePoint::Reader {
            spine_index,
            page_index,
            ..
        } => write!(
            status,
            "Saved chapter {}  page {}",
            spine_index + 1,
            page_index + 1
        )
        .map_err(|_| "reader sleep status overflowed")?,
        ResumePoint::Home { .. } => status
            .push_str("Home position saved")
            .map_err(|_| "reader sleep status overflowed")?,
        ResumePoint::Books { .. } => status
            .push_str("Books position saved")
            .map_err(|_| "reader sleep status overflowed")?,
        ResumePoint::Files { .. } => status
            .push_str("Files position saved")
            .map_err(|_| "reader sleep status overflowed")?,
        ResumePoint::Settings { .. } => status
            .push_str("Settings position saved")
            .map_err(|_| "reader sleep status overflowed")?,
        ResumePoint::Image { .. } => status
            .push_str("Image position saved")
            .map_err(|_| "reader sleep status overflowed")?,
    }

    for source in app.sleep_screen_plan(resume).sources() {
        match source {
            SleepScreenSource::CustomImage(image) => {
                let Some(file) = images.file(image) else {
                    continue;
                };
                match decode_image_frame(file, ScaleMode::Cover, store, workspaces) {
                    Ok(()) => {
                        esp_println::println!(
                            "BREWCTL/1 LOG stage=sleep-frame source=custom image={} crc32={:08x}",
                            image.index(),
                            crc32fast::hash(workspaces.frame_codec.frame())
                        );
                        return Ok(());
                    }
                    Err(error) => esp_println::println!(
                        "BREWCTL/1 LOG stage=sleep-frame source=custom state=fallback reason={}",
                        error
                    ),
                }
            }
            SleepScreenSource::BookCover(book) => {
                match decode_book_cover_frame(book, library, store, workspaces) {
                    Ok(true) => return Ok(()),
                    Ok(false) => {}
                    Err(error) => esp_println::println!(
                        "BREWCTL/1 LOG stage=sleep-frame source=cover state=fallback reason={}",
                        error
                    ),
                }
            }
            SleepScreenSource::BuiltIn => {
                let mut image =
                    PackedImage::new(frame_size(), READER_DEPTH, workspaces.frame_codec.frame())
                        .map_err(|_| "reader frame buffer has the wrong size")?;
                return render_app(
                    AppFrame::Sleep(SleepView::built_in(status.as_str(), app.battery())),
                    &mut image,
                )
                .map_err(|_| "reader sleep frame render failed");
            }
        }
    }
    Err("reader sleep plan has no renderable source")
}

fn render_error(
    app: &App,
    book: BookId,
    library: &DeviceLibrary,
    frame: &mut [u8; FRAME_BYTES],
) -> Result<(), &'static str> {
    let mut image = PackedImage::new(frame_size(), READER_DEPTH, frame)
        .map_err(|_| "reader frame buffer has the wrong size")?;
    render_app(
        AppFrame::Error {
            book_title: library.title(book),
            message: "This EPUB or chapter could not be opened.",
            battery: app.battery(),
        },
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
    let image = PackedBitmap::new(frame_size(), READER_DEPTH, bytes)
        .map_err(|_| "reader frame shape is invalid")?;
    let frequency = esp_hal::time::Rate::from_mhz(if image.is_monochrome() { 40 } else { 20 });
    let applied = store.with_device(|device| {
        device.with_hardware(|hardware| {
            let mut bus = hardware.display_bus_at(frequency).map_err(|error| {
                esp_println::println!(
                    "BREWCTL/1 LOG stage=display-session state=failed reason={:?}",
                    error
                );
                "reader display session failed"
            })?;
            panel
                .display
                .refresh_image(&mut bus, &mut Delay::new(), image, mode)
                .map_err(|error| {
                    esp_println::println!(
                        "BREWCTL/1 LOG stage=display-refresh state=failed reason={:?}",
                        error
                    );
                    "reader display refresh failed"
                })
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
    preferences: AppPreferences,
    library: &DeviceLibrary,
    store: &'static DeviceStore,
    panel: ReaderDisplay,
    rtc_resume: RtcResume,
) -> ! {
    let low_power = rtc_resume.write_resume(resume, preferences, library);
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

fn frame_size() -> Size {
    Size::new(480, 800).expect("the X4 frame dimensions are non-zero")
}

async fn stop(status: &'static str) -> ! {
    info!("{=str}; holding without retry", status);
    loop {
        core::future::pending::<()>().await;
    }
}
