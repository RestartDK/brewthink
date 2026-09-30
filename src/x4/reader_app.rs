use core::fmt::Write;

use defmt::info;
use embassy_futures::select::{Either, select};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use embassy_time::{Duration, Instant, Timer};
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
        App, AppEffect, AppInput, AppPreferences, AppView, BookId, BookProgress, Direction,
        HomeItem, ImageId, ReadingLocation, ResumePoint, SettingsItem, SleepScreenMode,
        SleepScreenSource,
    },
    bounded_layout::{BoundedPage, MAX_PAGE_LINES},
    bounded_xml::FixedString,
    chapter_cache::{ChapterRequest, ChapterWorkspace},
    cover::{
        COVER_BYTES, MAX_ENCODED_COVER_BYTES, SHELF_COVER_BYTES, bitmap, downsample_cover,
        shelf_bitmap,
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
    image::{PackedBitmap, PackedImage, READER_DEPTH, ScaleMode, Size},
    image_cache::{CacheSlot, CacheState, ImageKey, ImageSource, ImageSpec, ImageWorkspace},
    image_decoder::ImageFormat,
    image_viewer::render_image_viewer,
    input::{
        Button, ButtonDebouncer, ButtonEvent, ButtonTransition, PressedButtons,
        control::{ControlCommand, ControlLineBuffer},
    },
    library::ShelfBook,
    power::{BatteryEstimator, BatteryLevel, BatteryStatus},
    reader::{ReaderLine, ReaderStyle, ReaderView},
    settings::CustomImagePreview,
    sleep::SleepView,
    storage::{BookCatalog, BookFile, FatStorage, ImageFile, ReadOnlySdCard},
    transfer::{FileTransfer, UploadRequest, UploadTarget},
    ui::{AppFrame, render_app},
    x4::{X4FatBlockDevice, X4InputHardware, X4StorageHardware, decode_buttons},
    zip_stream::{InflateWorkspace, ZipValidationScratch},
};

const MAX_DEVICE_BOOKS: usize = 16;
const MAX_DEVICE_IMAGES: usize = 16;
const MAX_DEVICE_FILES: usize = MAX_DEVICE_BOOKS + MAX_DEVICE_IMAGES;
const MAX_CACHED_SPINE_PATHS: usize = 64;
const MAX_CACHED_SPINE_PATH_BYTES: usize = 2 * 1024;
const VISIBLE_COVER_SLOTS: usize = 4;
const FRAME_BYTES: usize = MONO_FRAME_BYTES * READER_DEPTH.bits();
const UPLOAD_CHUNK_BYTES: usize = 4 * 1024;
const UPLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
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

struct DeviceImages {
    files: [Option<ImageFile>; MAX_DEVICE_IMAGES],
    length: usize,
}

impl DeviceImages {
    const fn empty() -> Self {
        Self {
            files: [None; MAX_DEVICE_IMAGES],
            length: 0,
        }
    }

    fn file(&self, image: ImageId) -> Option<&ImageFile> {
        self.files.get(image.index()).and_then(Option::as_ref)
    }

    fn selected(&self, store: &DeviceStore) -> Option<ImageId> {
        let name = match store.app_data().read_selected_image() {
            Ok(name) => name,
            Err(error) => {
                info!(
                    "reader image selection unavailable: {}",
                    defmt::Display2Format(&error)
                );
                None
            }
        };
        name.and_then(|name| {
            self.files[..self.length]
                .iter()
                .flatten()
                .position(|image| *image.name() == name)
                .map(ImageId::new)
        })
        .or_else(|| (self.length > 0).then(|| ImageId::new(0)))
    }
}

#[derive(Clone, Copy)]
struct DeviceCatalogs<'a> {
    library: &'a DeviceLibrary,
    images: &'a DeviceImages,
}

type FrameCodecWorkspace = ImageWorkspace;

struct EpubWorkspace {
    inflate: InflateWorkspace,
    publication: DevicePublication,
    package: DevicePackageScratch,
}

impl EpubWorkspace {
    unsafe fn initialize(storage: *mut Self) {
        // SAFETY: each initializer writes its field in the aligned enclosing allocation.
        unsafe {
            InflateWorkspace::initialize_in_place(core::ptr::addr_of_mut!((*storage).inflate));
            DevicePublication::initialize_in_place(core::ptr::addr_of_mut!((*storage).publication));
            DevicePackageScratch::initialize_in_place(core::ptr::addr_of_mut!((*storage).package));
        }
    }
}

impl FrameCodecWorkspace {
    fn frame(&mut self) -> &mut [u8; FRAME_BYTES] {
        self.storage.bytes()
    }

    fn prepare_epub(
        &mut self,
    ) -> (
        &mut InflateWorkspace,
        &mut DevicePublication,
        &mut DevicePackageScratch,
    ) {
        // SAFETY: all fields are initialized before the enclosing value is exposed.
        let workspace = unsafe { self.storage.initialize(EpubWorkspace::initialize) };
        (
            &mut workspace.inflate,
            &mut workspace.publication,
            &mut workspace.package,
        )
    }
}

const CHAPTER_TITLE_WINDOW: usize = 16;

struct BookNavigation {
    book: Option<BookId>,
    first: usize,
    titles: [FixedString<{ crate::navigation::CHAPTER_TITLE_BYTES }>; CHAPTER_TITLE_WINDOW],
}

impl BookNavigation {
    const fn new() -> Self {
        Self {
            book: None,
            first: 0,
            titles: [FixedString::new(); CHAPTER_TITLE_WINDOW],
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
    frame_codec: &'static mut FrameCodecWorkspace,
    content: &'static mut ContentWorkspace,
    resource: &'static mut ChapterWorkspace,
}

#[derive(Clone, Copy)]
struct LoadedChapter {
    book: BookId,
    spine_index: usize,
    spine_count: usize,
    path: FixedString<128>,
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
    static NAVIGATION: ConstStaticCell<BookNavigation> =
        ConstStaticCell::new(BookNavigation::new());
    static FRAME_CODEC: ConstStaticCell<FrameCodecWorkspace> =
        ConstStaticCell::new(FrameCodecWorkspace::new());
    static CONTENT: ConstStaticCell<ContentWorkspace> =
        ConstStaticCell::new(ContentWorkspace::new());
    static RESOURCE: ConstStaticCell<ChapterWorkspace> =
        ConstStaticCell::new(ChapterWorkspace::new());
    static UPLOAD_BUFFER: ConstStaticCell<[u8; UPLOAD_CHUNK_BYTES]> =
        ConstStaticCell::new([0; UPLOAD_CHUNK_BYTES]);

    let mut card = ReadOnlySdCard::new(hardware);
    info!("reader startup: SD initialization start");
    if card.initialize().is_err() {
        stop("reader SD initialization failed").await;
    }
    info!("reader startup: SD initialization done");
    let store = STORE.init_with(|| {
        FatStorage::new(X4FatBlockDevice::new(card.enable_writes()), FixedTimeSource)
    });
    info!("reader startup: layout initialization start");
    if store.ensure_layout().is_err() {
        stop("reader SD layout initialization failed").await;
    }
    info!("reader startup: layout initialization done");
    let mut workspaces = Workspaces {
        navigation: NAVIGATION.take(),
        zip: ZIP.take(),
        frame_codec: FRAME_CODEC.take(),
        content: CONTENT.take(),
        resource: RESOURCE.take(),
    };
    info!("reader startup: workspace initialization done");
    let images = IMAGES.take();
    load_images(store, images);
    info!("reader startup: image scan done");
    let library = LIBRARY.take();
    info!("reader startup: book scan start");
    if load_library(store, library, &mut workspaces).is_err() {
        stop("reader /books scan failed").await;
    }
    info!("reader startup: book scan done");
    info!(
        "reader catalog ready: books={} images={} cached_spines={} path_bytes={}",
        library.length, images.length, library.spine_path_count, library.spine_path_byte_length
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
    let selected_image = images.selected(store);
    let (mut app, first_effect) = App::from_resume_with_catalog(
        library.length,
        images.length,
        selected_image,
        preferences,
        retained.resume,
    )
    .unwrap_or((
        App::with_catalog(library.length, images.length, selected_image, preferences),
        AppEffect::Render,
    ));
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
        Err(status) => stop(status).await,
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
                Err(status) => stop(status).await,
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
                ControlEvent::BooksChanged => {
                    loaded = None;
                    if load_library(store, library, &mut workspaces).is_err() {
                        esp_println::println!("BREWCTL/1 ERROR command=upload reason=catalog-read");
                        esp_println::println!("BREWCTL/1 DONE command=upload status=error");
                        Ok(None)
                    } else {
                        let effect = app.replace_book_catalog(library.length);
                        let result = run_effect(
                            effect,
                            &mut app,
                            DeviceCatalogs { library, images },
                            store,
                            &mut panel,
                            &mut workspaces,
                            &mut loaded,
                        );
                        esp_println::println!(
                            "BREWCTL/1 DONE command=upload status={}",
                            if result.is_ok() { "ok" } else { "error" }
                        );
                        result
                    }
                }
                ControlEvent::ImagesChanged => {
                    load_images(store, images);
                    let selected = images.selected(store);
                    let effect = app.replace_image_catalog(images.length, selected);
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
            };
            match result {
                Ok(Some(resume)) => {
                    enter_sleep(resume, app.preferences(), library, store, panel, rtc_resume).await;
                }
                Ok(None) => {}
                Err(status) => stop(status).await,
            }
        }

        let next_control_poll = Timer::after(if control_runtime.is_upload_active() {
            Duration::from_micros(250)
        } else {
            Duration::from_millis(20)
        });
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
                if self.app.preferences() != previous_preferences {
                    if self
                        .store
                        .app_data()
                        .write_preferences(self.app.preferences())
                        .is_err()
                    {
                        esp_println::println!(
                            "BREWCTL/1 LOG stage=preferences state=volatile reason=app-data-unavailable"
                        );
                    } else {
                        esp_println::println!("BREWCTL/1 LOG stage=preferences state=persisted");
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
    BooksChanged,
}

struct UsbControlRuntime<'a> {
    lines: ControlLineBuffer,
    transfer: FileTransfer,
    upload_buffer: &'a mut [u8; UPLOAD_CHUNK_BYTES],
    upload_buffer_length: usize,
    last_activity: Instant,
}

impl<'a> UsbControlRuntime<'a> {
    fn new(upload_buffer: &'a mut [u8; UPLOAD_CHUNK_BYTES]) -> Self {
        Self {
            lines: ControlLineBuffer::new(),
            transfer: FileTransfer::new(),
            upload_buffer,
            upload_buffer_length: 0,
            last_activity: Instant::now(),
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
        if self.transfer.is_active() && self.last_activity.elapsed() >= UPLOAD_IDLE_TIMEOUT {
            self.abort_upload(store, "timeout");
        }

        while let Ok(byte) = control.read_byte() {
            self.last_activity = Instant::now();
            if self.transfer.is_active() {
                self.upload_buffer[self.upload_buffer_length] = byte;
                self.upload_buffer_length += 1;
                let request = self
                    .transfer
                    .request()
                    .expect("active upload has a request");
                let remaining = request.length() - self.transfer.received();
                if self.upload_buffer_length == remaining.min(UPLOAD_CHUNK_BYTES)
                    && let Some(event) = self.flush_upload_chunk(store)
                {
                    return Some(event);
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
                Ok(ControlCommand::DropImageCache) => match store.app_data().drop_image_cache() {
                    Ok(removed) => {
                        esp_println::println!("BREWCTL/1 DROPPED images={removed}");
                        esp_println::println!("BREWCTL/1 DONE command=drop-image-cache status=ok");
                    }
                    Err(error) => {
                        esp_println::println!(
                            "BREWCTL/1 ERROR command=drop-image-cache reason={error}"
                        );
                        esp_println::println!(
                            "BREWCTL/1 DONE command=drop-image-cache status=error"
                        );
                    }
                },
                Ok(ControlCommand::Verify(request)) => {
                    match store.app_data().verify_upload(request, self.upload_buffer) {
                        Ok(()) => {
                            esp_println::println!(
                                "BREWCTL/1 VERIFIED kind={} name={} bytes={} crc32={:08x}",
                                request.target().kind(),
                                request.target().name(),
                                request.length(),
                                request.crc32()
                            );
                            esp_println::println!("BREWCTL/1 DONE command=verify status=ok");
                        }
                        Err(_) => {
                            esp_println::println!(
                                "BREWCTL/1 ERROR command=verify reason=file-mismatch-or-storage"
                            );
                            esp_println::println!("BREWCTL/1 DONE command=verify status=error");
                        }
                    }
                }
                Ok(ControlCommand::Upload(request)) => {
                    if matches!(request.target(), UploadTarget::Book(_))
                        && !matches!(app.view(), AppView::Home(_))
                    {
                        esp_println::println!(
                            "BREWCTL/1 ERROR command=upload reason=return-home-first"
                        );
                        esp_println::println!("BREWCTL/1 DONE command=upload status=error");
                    } else {
                        self.begin_upload(request, store);
                    }
                }
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
                self.last_activity = Instant::now();
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

    fn flush_upload_chunk(&mut self, store: &DeviceStore) -> Option<ControlEvent> {
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
            return None;
        }
        self.upload_buffer_length = 0;
        self.last_activity = Instant::now();
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
            return None;
        }
        match self.transfer.finish(&mut app_data, self.upload_buffer) {
            Ok(request) => match request.target() {
                UploadTarget::Image(_) => {
                    esp_println::println!("BREWCTL/1 DONE command=upload status=ok");
                    Some(ControlEvent::ImagesChanged)
                }
                UploadTarget::Book(_) => Some(ControlEvent::BooksChanged),
            },
            Err(error) => {
                esp_println::println!("BREWCTL/1 ERROR command=upload reason={}", error.name());
                esp_println::println!("BREWCTL/1 DONE command=upload status=error");
                None
            }
        }
    }

    fn abort_upload(&mut self, store: &DeviceStore, reason: &str) {
        let mut app_data = store.app_data();
        let was_active = self.transfer.is_active();
        let result = self.transfer.abort(&mut app_data);
        self.upload_buffer_length = 0;
        self.last_activity = Instant::now();
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

fn load_library(
    store: &DeviceStore,
    library: &mut DeviceLibrary,
    workspaces: &mut Workspaces,
) -> Result<(), ()> {
    info!("reader startup: book directory scan start");
    let catalog = workspaces.content.prepare_catalog();
    store.scan_into(catalog).map_err(|error| {
        info!(
            "reader book catalog unavailable: {}",
            defmt::Display2Format(&error)
        );
    })?;
    info!(
        "reader startup: book directory scan done entries={}",
        catalog.len()
    );
    for (catalog_index, file) in catalog.books().copied().enumerate() {
        if library.files[..library.length].contains(&Some(file)) {
            continue;
        }
        if library.length == MAX_DEVICE_BOOKS {
            break;
        }
        info!(
            "reader startup: book validation start index={} bytes={}",
            catalog_index,
            file.size()
        );
        let (inflate, publication, package) = workspaces.frame_codec.prepare_epub();
        let reader = match store.open_reader(file) {
            Ok(reader) => reader,
            Err(_) => continue,
        };
        info!("reader startup: book open done index={}", catalog_index);
        let book = match DeviceEpub::open(
            reader,
            workspaces.zip,
            package,
            inflate,
            workspaces.resource.bytes(),
            publication,
        ) {
            Ok(book) => book,
            Err(_) => continue,
        };
        info!(
            "reader startup: book validation done index={}",
            catalog_index
        );
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

fn load_images(store: &DeviceStore, images: &mut DeviceImages) {
    *images = DeviceImages::empty();
    let catalog = match store.app_data().scan_images::<MAX_DEVICE_IMAGES>() {
        Ok(catalog) => catalog,
        Err(error) => {
            info!(
                "reader image catalog unavailable: {}",
                defmt::Display2Format(&error)
            );
            return;
        }
    };
    for image in catalog.images() {
        images.files[images.length] = Some(image);
        images.length += 1;
    }
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
                DISPLAYED_FRAME.init_with(|| [0xFF; MONO_FRAME_BYTES]),
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

fn run_effect(
    mut effect: AppEffect,
    app: &mut App,
    catalogs: DeviceCatalogs<'_>,
    store: &DeviceStore,
    panel: &mut ReaderDisplay,
    workspaces: &mut Workspaces,
    loaded: &mut Option<LoadedChapter>,
) -> Result<Option<ResumePoint>, &'static str> {
    let library = catalogs.library;
    let images = catalogs.images;
    loop {
        effect = match effect {
            AppEffect::None => return Ok(None),
            AppEffect::LoadProgress { book, origin } => {
                let stored = match (DeviceProgress { library, store }).load(book) {
                    Ok(stored) => stored,
                    Err(error) => {
                        error.report("read");
                        None
                    }
                };
                app.progress_loaded(book, origin, stored)
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
                let next = match load_chapter(
                    book,
                    spine_index,
                    spine_index,
                    library,
                    store,
                    workspaces,
                ) {
                    Ok(chapter) => {
                        *loaded = Some(chapter);
                        let page = workspaces.content.prepare_page();
                        let file = library.file(book).ok_or("reader book is missing")?;
                        match prepare_chapter_page(
                            ChapterRequest {
                                book: &file,
                                path: chapter.path.as_str(),
                                page_index: 0,
                                preferences: app.reader_preferences(),
                            },
                            store,
                            workspaces.resource,
                            workspaces.frame_codec,
                            workspaces.zip,
                            page,
                        ) {
                            Ok(page_count) => {
                                app.chapter_loaded(chapter.spine_count, page_count)
                                    .map_err(|_| "reader application state rejected chapter")?
                            }
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
            AppEffect::Render => match app.view() {
                AppView::BookCover { book, .. } => {
                    if decode_book_cover_frame(book, library, store, workspaces).unwrap_or(false) {
                        refresh(store, panel, workspaces.frame_codec.frame())?;
                        return Ok(None);
                    }
                    app.input(AppInput::Confirm)
                }
                AppView::Home(_) => {
                    render_home_frame(app, workspaces.frame_codec.frame())?;
                    refresh(store, panel, workspaces.frame_codec.frame())?;
                    return Ok(None);
                }
                AppView::Library => {
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
                AppView::Files(_) => {
                    render_files_frame(app, library, images, workspaces.frame_codec.frame())?;
                    refresh(store, panel, workspaces.frame_codec.frame())?;
                    return Ok(None);
                }
                AppView::Settings(_) => {
                    render_settings_frame(app, images, store, workspaces)?;
                    refresh(store, panel, workspaces.frame_codec.frame())?;
                    return Ok(None);
                }
                AppView::Loading(_) => return Err("reader render requested while loading"),
                AppView::Reader(_) | AppView::ReaderDrawer(_) => {
                    let location = match app.view() {
                        AppView::Reader(session) => session.location(),
                        AppView::ReaderDrawer(drawer) => drawer.session().location(),
                        _ => unreachable!(),
                    };
                    esp_println::println!(
                        "BREWCTL/1 LOG stage=render-reader state=start book={} spine={} page={}",
                        location.book().index(),
                        location.spine_index(),
                        location.page_index()
                    );
                    let title_index = match app.view() {
                        AppView::ReaderDrawer(drawer) => drawer.chapter(),
                        _ => location.spine_index(),
                    };
                    let chapter = match *loaded {
                        Some(chapter)
                            if chapter.book == location.book()
                                && chapter.spine_index == location.spine_index()
                                && workspaces.navigation.book == Some(location.book())
                                && (workspaces.navigation.first
                                    ..workspaces.navigation.first + CHAPTER_TITLE_WINDOW)
                                    .contains(&title_index) =>
                        {
                            chapter
                        }
                        _ => {
                            let chapter = load_chapter(
                                location.book(),
                                location.spine_index(),
                                title_index,
                                library,
                                store,
                                workspaces,
                            )
                            .map_err(|_| "reader chapter metadata reload failed")?;
                            *loaded = Some(chapter);
                            chapter
                        }
                    };
                    render_page(app, location, library, chapter, store, workspaces)?;
                    refresh(store, panel, workspaces.frame_codec.frame())?;
                    if let Err(error) =
                        (DeviceProgress { library, store }).save(app.reading_checkpoint())
                    {
                        error.report("write");
                    }
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
                AppView::Image(image) => {
                    render_image_frame(app, image, images, store, workspaces)?;
                    refresh(store, panel, workspaces.frame_codec.frame())?;
                    return Ok(None);
                }
                AppView::Error { book, .. } => {
                    esp_println::println!("BREWCTL/1 LOG stage=render-error state=start");
                    render_error(app, book, library, workspaces.frame_codec.frame())?;
                    refresh(store, panel, workspaces.frame_codec.frame())?;
                    esp_println::println!("BREWCTL/1 LOG stage=render-error state=done");
                    return Ok(None);
                }
                AppView::Sleeping { resume } => {
                    esp_println::println!("BREWCTL/1 LOG stage=render-sleep state=start");
                    render_sleep_frame(app, resume, library, images, store, workspaces)?;
                    refresh(store, panel, workspaces.frame_codec.frame())?;
                    esp_println::println!("BREWCTL/1 LOG stage=render-sleep state=done");
                    info!("reader retained sleep frame refreshed");
                    app.sleep_frame_ready()
                        .map_err(|_| "reader application state rejected sleep frame")?
                }
            },
            AppEffect::EnterDeepSleep { resume } => {
                if let Err(error) =
                    (DeviceProgress { library, store }).save(app.reading_checkpoint())
                {
                    error.report("write");
                }
                return Ok(Some(resume));
            }
        };
    }
}

struct DeviceProgress<'a> {
    library: &'a DeviceLibrary,
    store: &'a DeviceStore,
}

#[derive(Debug)]
enum DeviceProgressError {
    MissingBook,
    Storage(
        crate::storage::AppDataError<
            <X4FatBlockDevice<'static> as embedded_sdmmc::BlockDevice>::Error,
        >,
    ),
}

impl DeviceProgressError {
    fn report(&self, operation: &str) {
        match self {
            Self::MissingBook => info!("progress book is missing"),
            Self::Storage(error) => {
                info!("progress storage failed: {}", defmt::Debug2Format(error))
            }
        }
        esp_println::println!(
            "BREWCTL/1 ERROR command=progress reason=storage operation={operation}"
        );
    }
}

impl DeviceProgress<'_> {
    fn load(&self, book: BookId) -> Result<Option<BookProgress>, DeviceProgressError> {
        let file = self
            .library
            .file(book)
            .ok_or(DeviceProgressError::MissingBook)?;
        self.store
            .app_data()
            .read_book_progress(&file)
            .map_err(DeviceProgressError::Storage)
    }

    fn save(&self, checkpoint: Option<(BookId, BookProgress)>) -> Result<(), DeviceProgressError> {
        let Some((book, progress)) = checkpoint else {
            return Ok(());
        };
        match self.load(book) {
            Ok(Some(stored)) if stored == progress => return Ok(()),
            Ok(Some(_) | None) => {}
            Err(error) => error.report("read"),
        }
        let file = self
            .library
            .file(book)
            .ok_or(DeviceProgressError::MissingBook)?;
        self.store
            .app_data()
            .write_book_progress(&file, progress)
            .map_err(DeviceProgressError::Storage)
    }
}

fn load_chapter(
    selected: BookId,
    spine_index: usize,
    title_index: usize,
    library: &DeviceLibrary,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<LoadedChapter, ()> {
    let navigation_matches = workspaces.navigation.book == Some(selected)
        && (workspaces.navigation.first..workspaces.navigation.first + CHAPTER_TITLE_WINDOW)
            .contains(&title_index);
    if let Some(path) = library
        .spine_path(selected, spine_index)
        .filter(|_| navigation_matches)
    {
        return Ok(LoadedChapter {
            book: selected,
            spine_index,
            spine_count: library.spine_count(selected),
            path: FixedString::try_from_str(path).map_err(|_| ())?,
        });
    }
    let file = library.file(selected).ok_or(())?;
    let (inflate, publication, package) = workspaces.frame_codec.prepare_epub();
    let reader = store.open_reader(file).map_err(|_| ())?;
    let book = DeviceEpub::open(
        reader,
        workspaces.zip,
        package,
        inflate,
        workspaces.resource.bytes(),
        publication,
    )
    .map_err(|_| ())?;
    if !navigation_matches {
        workspaces.navigation.first = title_index / CHAPTER_TITLE_WINDOW * CHAPTER_TITLE_WINDOW;
        if let Err(error) = book.read_chapter_titles(
            &mut workspaces.navigation.titles,
            workspaces.navigation.first,
            workspaces.resource.bytes(),
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
    Ok(LoadedChapter {
        book: selected,
        spine_index,
        spine_count: book.publication().spine_len(),
        path: FixedString::try_from_str(
            book.publication().spine_item(spine_index).ok_or(())?.path(),
        )
        .map_err(|_| ())?,
    })
}

fn prepare_chapter_page(
    request: ChapterRequest<'_>,
    store: &DeviceStore,
    resource: &mut ChapterWorkspace,
    images: &mut FrameCodecWorkspace,
    zip: &mut ZipValidationScratch,
    page: &mut BoundedPage,
) -> Result<usize, &'static str> {
    let started = Instant::now();
    match store
        .app_data()
        .chapter_page(request, resource, images, zip, page)
    {
        Ok(chapter) => {
            esp_println::println!(
                "BREWCTL/1 LOG stage=chapter-cache state={} pages={} elapsed_ms={}",
                match chapter.state {
                    CacheState::Hit => "hit",
                    CacheState::Prepared => "prepared",
                },
                chapter.summary.page_count,
                started.elapsed().as_millis()
            );
            Ok(chapter.summary.page_count)
        }
        Err(error) => {
            esp_println::println!(
                "BREWCTL/1 LOG stage=chapter-cache state=failed reason={:?}",
                error
            );
            Err("reader chapter cache preparation failed")
        }
    }
}

fn prepare_image(
    source: ImageSource<'_>,
    spec: ImageSpec,
    store: &DeviceStore,
    codec: &mut ImageWorkspace,
    zip: &mut ZipValidationScratch,
    output: &mut [u8],
    protected: &[CacheSlot],
) -> Result<(), &'static str> {
    let started = Instant::now();
    esp_println::println!(
        "BREWCTL/1 LOG stage=image-cache state=start width={} height={}",
        spec.size().width(),
        spec.size().height()
    );
    match store
        .app_data()
        .prepare_pinned_image(source, spec, codec, zip, output, protected)
    {
        Ok(image) => {
            let state = match image.state {
                CacheState::Hit => "hit",
                CacheState::Prepared => "prepared",
            };
            esp_println::println!(
                "BREWCTL/1 LOG stage=image-cache state={} width={} height={} source_width={} source_height={} ms={}",
                state,
                spec.size().width(),
                spec.size().height(),
                image.source.width(),
                image.source.height(),
                started.elapsed().as_millis()
            );
            Ok(())
        }
        Err(error) => {
            esp_println::println!(
                "BREWCTL/1 LOG stage=image-cache state=failed reason={:?}",
                error
            );
            Err("reader image preparation failed")
        }
    }
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
    let file = library.file(selected).ok_or("reader book is missing")?;
    let spec = ImageSpec::new(
        crate::cover::COVER_WIDTH,
        crate::cover::COVER_HEIGHT,
        ScaleMode::Cover,
    )
    .expect("cover dimensions");
    prepare_image(
        ImageSource::Book { file: &file, path },
        spec,
        store,
        workspaces.frame_codec,
        workspaces.zip,
        workspaces.content.cover(),
        &[],
    )?;
    Ok(true)
}

fn decode_book_cover_frame(
    selected: BookId,
    library: &DeviceLibrary,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<bool, &'static str> {
    let Some(path) = library.cover_path(selected) else {
        return Ok(false);
    };
    let file = library.file(selected).ok_or("reader book is missing")?;
    let spec = ImageSpec::new(480, 800, ScaleMode::Contain).expect("frame dimensions");
    prepare_image(
        ImageSource::Book { file: &file, path },
        spec,
        store,
        workspaces.frame_codec,
        workspaces.zip,
        &mut workspaces.resource.bytes()[..FRAME_BYTES],
        &[],
    )?;
    workspaces
        .frame_codec
        .frame()
        .copy_from_slice(&workspaces.resource.bytes()[..FRAME_BYTES]);
    Ok(true)
}

fn decode_image_preview(
    file: &ImageFile,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), ()> {
    let spec = ImageSpec::new(
        crate::cover::COVER_WIDTH,
        crate::cover::COVER_HEIGHT,
        ScaleMode::Cover,
    )
    .expect("cover dimensions");
    prepare_image(
        ImageSource::File(file),
        spec,
        store,
        workspaces.frame_codec,
        workspaces.zip,
        workspaces.content.cover(),
        &[],
    )
    .map_err(|_| ())
}

fn decode_image_frame(
    file: &ImageFile,
    scale: ScaleMode,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), &'static str> {
    let spec = ImageSpec::new(480, 800, scale).expect("frame dimensions");
    prepare_image(
        ImageSource::File(file),
        spec,
        store,
        workspaces.frame_codec,
        workspaces.zip,
        &mut workspaces.resource.bytes()[..FRAME_BYTES],
        &[],
    )?;
    workspaces
        .frame_codec
        .frame()
        .copy_from_slice(&workspaces.resource.bytes()[..FRAME_BYTES]);
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
            Err(()) => CustomImagePreview::Invalid,
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
    if decode_image_frame(file, ScaleMode::Contain, store, workspaces).is_err() {
        let mut target =
            PackedImage::new(frame_size(), READER_DEPTH, workspaces.frame_codec.frame())
                .map_err(|_| "reader frame buffer has the wrong size")?;
        return render_app(
            AppFrame::Error {
                book_title: file.name().as_str(),
                message: "This image could not be opened.",
                battery: app.battery(),
            },
            &mut target,
        )
        .map_err(|_| "reader image error render failed");
    }
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
        decoded[slot] =
            decode_book_cover(BookId::new(index), library, store, workspaces).unwrap_or(false);
        if decoded[slot] {
            let cache_slot = slot - usize::from(selected_slot < slot);
            let offset = MAX_ENCODED_COVER_BYTES as usize + cache_slot * SHELF_COVER_BYTES;
            downsample_cover(
                workspaces.content.cover(),
                &mut workspaces.resource.bytes()[offset..offset + SHELF_COVER_BYTES],
            );
        }
    }
    if let Some(index) = selected.filter(|index| visible.contains(index)) {
        decoded[index - visible.start] =
            decode_book_cover(BookId::new(index), library, store, workspaces).unwrap_or(false);
    }
    let covers = &workspaces.resource.bytes()[MAX_ENCODED_COVER_BYTES as usize..];
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
    chapter: LoadedChapter,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), &'static str> {
    let file = library
        .file(location.book())
        .ok_or("reader book is missing")?;
    let page = workspaces.content.prepare_page();
    prepare_chapter_page(
        ChapterRequest {
            book: &file,
            path: chapter.path.as_str(),
            page_index: location.page_index(),
            preferences: app.reader_preferences(),
        },
        store,
        workspaces.resource,
        workspaces.frame_codec,
        workspaces.zip,
        page,
    )?;
    let mut protected = [CacheSlot::default(); MAX_PAGE_LINES];
    let mut protected_count = 0;
    for image in page.images() {
        if let Some(key) = ImageKey::resource(&file, image.resource(), image.spec()) {
            protected[protected_count] = key.slot();
            protected_count += 1;
        }
    }
    for image in page.images() {
        let _ = prepare_image(
            ImageSource::Book {
                file: &file,
                path: image.path(),
            },
            image.spec(),
            store,
            workspaces.frame_codec,
            workspaces.zip,
            &mut workspaces.resource.bytes()[..image.spec().byte_len()],
            &protected[..protected_count],
        );
    }
    let mut lines = [ReaderLine::new("", ReaderStyle::Body); MAX_PAGE_LINES];
    let mut line_count = 0;
    for line in page.lines() {
        lines[line_count] = ReaderLine::new(line.text(), line.style()).with_top(line.top() as u16);
        line_count += 1;
    }
    let chapter_index = match app.view() {
        AppView::ReaderDrawer(drawer) => drawer.chapter(),
        _ => location.spine_index(),
    };
    let mut fallback = FixedString::<64>::new();
    write!(fallback, "Chapter {}", chapter_index + 1).ok();
    let chapter_title = chapter_index
        .checked_sub(workspaces.navigation.first)
        .and_then(|index| workspaces.navigation.titles.get(index))
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
    let mut target = PackedImage::new(frame_size(), READER_DEPTH, workspaces.frame_codec.frame())
        .map_err(|_| "reader frame buffer has the wrong size")?;
    let mut drawn = 0;
    crate::reader::render_reader_with_images(view, &mut target, |target, offset| {
        for image in page.images() {
            let pixels = &mut workspaces.resource.bytes()[..image.spec().byte_len()];
            let bitmap = ImageKey::resource(&file, image.resource(), image.spec())
                .filter(|key| store.app_data().read_prepared_image(key, pixels).is_ok())
                .and_then(|_| PackedBitmap::new(image.spec().size(), READER_DEPTH, pixels).ok());
            if crate::reader::render_inline_image(image, bitmap, target, offset) {
                drawn += 1;
            }
        }
    })
    .map_err(|_| "reader page render failed")?;
    esp_println::println!(
        "BREWCTL/1 LOG stage=inline-images state=done images={} drawn={}",
        page.images().count(),
        drawn
    );
    Ok(())
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
                if decode_image_frame(file, ScaleMode::Cover, store, workspaces).is_ok() {
                    esp_println::println!(
                        "BREWCTL/1 LOG stage=sleep-frame source=custom image={} crc32={:08x}",
                        image.index(),
                        crc32fast::hash(workspaces.frame_codec.frame())
                    );
                    return Ok(());
                }
            }
            SleepScreenSource::BookCover(book) => {
                if decode_book_cover_frame(book, library, store, workspaces).unwrap_or(false) {
                    esp_println::println!(
                        "BREWCTL/1 LOG stage=sleep-frame source=cover book={} crc32={:08x}",
                        book.index(),
                        crc32fast::hash(workspaces.frame_codec.frame())
                    );
                    return Ok(());
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
    let started = embassy_time::Instant::now();
    let mode = panel.refresh_policy.requested_mode();
    let image = PackedBitmap::new(frame_size(), READER_DEPTH, bytes)
        .map_err(|_| "reader frame shape is invalid")?;
    let monochrome = image.is_monochrome();
    let frequency = esp_hal::time::Rate::from_mhz(if monochrome { 40 } else { 20 });
    let applied = store.with_device(|device| {
        device.with_hardware(|hardware| {
            let mut bus = hardware
                .display_bus_at(frequency)
                .map_err(|_| "reader display session failed")?;
            panel
                .display
                .refresh_image(&mut bus, &mut Delay::new(), image, mode)
                .map_err(|_| "reader display refresh failed")
        })
    })?;
    panel.refresh_policy.commit(applied);
    esp_println::println!(
        "BREWCTL/1 LOG stage=display-refresh applied={} pixels={} elapsed_ms={}",
        applied.name(),
        if monochrome {
            "monochrome"
        } else {
            "grayscale"
        },
        started.elapsed().as_millis()
    );
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
