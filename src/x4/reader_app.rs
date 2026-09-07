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

use crate::{
    app::{
        App, AppEffect, AppInput, AppPreferences, AppView, BookId, BookOrigin, Direction, HomeItem,
        ImageId, ReadingLocation, ResumePoint, SettingsItem, SleepScreenMode, SleepScreenSource,
    },
    bounded_layout::{BoundedPage, MAX_PAGE_LINES, layout_xhtml_page_into},
    bounded_xml::FixedString,
    cover::{
        COVER_BYTES, CoverDecodeWorkspace, JpegDecodeWorkspace, MAX_ENCODED_COVER_BYTES, bitmap,
        decode_jpeg_cover, decode_png_cover, encoded_cover_fits,
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
const SHELF_COVER_WIDTH: usize = 88;
const SHELF_COVER_HEIGHT: usize = 132;
const SHELF_COVER_BYTES: usize = SHELF_COVER_WIDTH * SHELF_COVER_HEIGHT / 8 * READER_DEPTH.bits();
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
    preferences: AppPreferences,
}

const RESUME_MAGIC: u32 = 0x4257_5233;
const HOME_KIND: u32 = 1;
const BOOKS_KIND: u32 = 2;
const FILES_KIND: u32 = 3;
const SETTINGS_KIND: u32 = 4;
const READER_KIND: u32 = 5;
const IMAGE_KIND: u32 = 6;

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut RETAINED_RESUME: [u32; 8] = [0; 8];

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
    static STORE: StaticCell<DeviceStore> = StaticCell::new();
    static LIBRARY: ConstStaticCell<DeviceLibrary> = ConstStaticCell::new(DeviceLibrary::empty());
    static IMAGES: ConstStaticCell<DeviceImages> = ConstStaticCell::new(DeviceImages::empty());
    static ZIP: ConstStaticCell<ZipValidationScratch> =
        ConstStaticCell::new(ZipValidationScratch::new());
    static PACKAGE: ConstStaticCell<DevicePackageScratch> =
        ConstStaticCell::new(DevicePackageScratch::new());
    static FRAME_CODEC: ConstStaticCell<FrameCodecWorkspace> =
        ConstStaticCell::new(FrameCodecWorkspace::new());
    static CONTENT: ConstStaticCell<ContentWorkspace> =
        ConstStaticCell::new(ContentWorkspace::new());
    static RESOURCE: ConstStaticCell<[u8; MAX_DEVICE_RESOURCE_BYTES]> =
        ConstStaticCell::new([0; MAX_DEVICE_RESOURCE_BYTES]);
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
        zip: ZIP.take(),
        package: PACKAGE.take(),
        frame_codec: FRAME_CODEC.take(),
        content: CONTENT.take(),
        resource: RESOURCE.take(),
    };
    info!("reader startup: workspace initialization done");
    let library = LIBRARY.take();
    info!("reader startup: book scan start");
    if load_library(store, library, &mut workspaces).is_err() {
        stop("reader /books scan failed").await;
    }
    info!("reader startup: book scan done");
    let images = IMAGES.take();
    load_images(store, images);
    info!("reader startup: image scan done");
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
    let retained = read_resume().unwrap_or(RetainedApp {
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
            enter_sleep(resume, app.preferences(), store, panel, low_power).await;
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
                    enter_sleep(resume, app.preferences(), store, panel, low_power).await;
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
    store.scan_into(catalog).map_err(|_| ())?;
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
        let (inflate, publication) = workspaces.frame_codec.prepare_epub();
        let reader = match store.open_reader(file) {
            Ok(reader) => reader,
            Err(_) => continue,
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
    let Ok(catalog) = store.app_data().scan_images::<MAX_DEVICE_IMAGES>() else {
        return;
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
                        let page = workspaces.content.prepare_page();
                        match layout_xhtml_page_into(
                            &workspaces.resource[..chapter.length],
                            0,
                            app.reader_preferences(),
                            page,
                        ) {
                            Ok(()) => app
                                .chapter_loaded(chapter.spine_count, page.page_count())
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
            AppEffect::Render => match app.view() {
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
                AppView::Reader(session) => {
                    let location = session.location();
                    esp_println::println!(
                        "BREWCTL/1 LOG stage=render-reader state=start book={} spine={} page={}",
                        location.book().index(),
                        location.spine_index(),
                        location.page_index()
                    );
                    let chapter = loaded.ok_or("reader chapter was not loaded")?;
                    if chapter.book != location.book()
                        || chapter.spine_index != location.spine_index()
                    {
                        return Err("reader chapter cache mismatch");
                    }
                    let xhtml = &workspaces.resource[..chapter.length];
                    render_page(
                        app,
                        location,
                        library,
                        xhtml,
                        workspaces.content.prepare_page(),
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
            AppEffect::EnterDeepSleep { resume } => return Ok(Some(resume)),
        };
    }
}

fn load_chapter(
    selected: BookId,
    spine_index: usize,
    library: &DeviceLibrary,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<LoadedChapter, ()> {
    let file = library.file(selected).ok_or(())?;
    let (inflate, publication) = workspaces.frame_codec.prepare_epub();
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
            publication,
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
        .read_entry(
            entry,
            &mut workspaces.resource[..MAX_ENCODED_COVER_BYTES as usize],
            inflate,
        )
        .map_err(|_| "reader cover read failed")?;
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
    decoded.map_err(|_| "reader cover decode failed")?;
    esp_println::println!(
        "BREWCTL/1 LOG stage=cover state=done book={}",
        selected.index()
    );
    Ok(true)
}

fn decode_image_preview(
    file: &ImageFile,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), ()> {
    let loaded = store
        .app_data()
        .read_image(
            *file.name(),
            &mut workspaces.resource[..MAX_DEVICE_IMAGE_BYTES],
        )
        .map_err(|_| ())?;
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
    decoded.map(|_| ()).map_err(|_| ())
}

fn decode_image_frame(
    file: &ImageFile,
    scale: ScaleMode,
    store: &DeviceStore,
    workspaces: &mut Workspaces,
) -> Result<(), &'static str> {
    let (encoded_buffer, decoder_buffer) = workspaces.resource.split_at_mut(MAX_DEVICE_IMAGE_BYTES);
    let loaded = store
        .app_data()
        .read_image(*file.name(), encoded_buffer)
        .map_err(|_| "reader image read failed")?;
    let encoded = &encoded_buffer[..loaded.length()];
    let frame = workspaces.frame_codec.frame();
    let mut target = PackedImage::new(frame_size(), READER_DEPTH, frame)
        .map_err(|_| "reader frame buffer has the wrong size")?;
    let options = RenderOptions {
        scale,
        dither: Dither::None,
    };
    match loaded.format() {
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
    .map_err(|_| "reader image decode failed")?;
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
                &mut workspaces.resource[offset..offset + SHELF_COVER_BYTES],
            );
        }
    }
    if let Some(index) = selected.filter(|index| visible.contains(index)) {
        decoded[index - visible.start] =
            decode_book_cover(BookId::new(index), library, store, workspaces).unwrap_or(false);
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
    frame: &mut [u8; FRAME_BYTES],
) -> Result<(), &'static str> {
    layout_xhtml_page_into(xhtml, location.page_index(), app.reader_preferences(), page)
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
        app.reader_preferences(),
        app.battery(),
    );
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
        ResumePoint::Image { .. } => status
            .push_str("IMAGE POSITION SAVED")
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
                if !decode_book_cover(book, library, store, workspaces).unwrap_or(false) {
                    continue;
                }
                let mut image =
                    PackedImage::new(frame_size(), READER_DEPTH, workspaces.frame_codec.frame())
                        .map_err(|_| "reader frame buffer has the wrong size")?;
                return render_app(
                    AppFrame::Sleep(SleepView::book_cover(
                        library.title(book),
                        library.creator(book),
                        status.as_str(),
                        bitmap(workspaces.content.cover()),
                        app.battery(),
                    )),
                    &mut image,
                )
                .map_err(|_| "reader sleep frame render failed");
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
    let preferences = AppPreferences::from_packed(record.preferences)?;
    let selected_index = || {
        if record.primary == u32::MAX {
            Some(None)
        } else {
            Some(Some(usize::try_from(record.primary).ok()?))
        }
    };
    let resume = match record.kind {
        HOME_KIND => ResumePoint::Home {
            selected: HomeItem::from_index(usize::try_from(record.primary).ok()?)?,
        },
        BOOKS_KIND => ResumePoint::Books {
            selected: selected_index()?.map(BookId::new),
        },
        FILES_KIND => ResumePoint::Files {
            selected: selected_index()?.map(crate::app::FileId::new),
        },
        SETTINGS_KIND => ResumePoint::Settings {
            selected: SettingsItem::from_index(usize::try_from(record.primary).ok()?)?,
            draft: AppPreferences::from_packed(record.detail)?,
        },
        READER_KIND => ResumePoint::Reader {
            book: BookId::new(usize::try_from(record.primary).ok()?),
            spine_index: usize::try_from(record.secondary).ok()?,
            page_index: usize::try_from(record.tertiary).ok()?,
            origin: BookOrigin::from_index(usize::try_from(record.detail).ok()?)?,
        },
        IMAGE_KIND => ResumePoint::Image {
            image: ImageId::new(usize::try_from(record.primary).ok()?),
        },
        _ => return None,
    };
    Some(RetainedApp {
        resume,
        preferences,
    })
}

fn write_resume(resume: ResumePoint, preferences: AppPreferences) {
    let (kind, primary, secondary, tertiary, detail) = match resume {
        ResumePoint::Home { selected } => (HOME_KIND, selected.index() as u32, 0, 0, 0),
        ResumePoint::Books { selected } => (
            BOOKS_KIND,
            packed_index(selected.map(BookId::index)),
            0,
            0,
            0,
        ),
        ResumePoint::Files { selected } => (
            FILES_KIND,
            packed_index(selected.map(crate::app::FileId::index)),
            0,
            0,
            0,
        ),
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
        ResumePoint::Image { image } => (
            IMAGE_KIND,
            u32::try_from(image.index()).unwrap_or(u32::MAX),
            0,
            0,
            0,
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

fn packed_index(index: Option<usize>) -> u32 {
    index
        .and_then(|index| u32::try_from(index).ok())
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

fn downsample_cover(source: &[u8; COVER_BYTES], output: &mut [u8]) {
    let source = bitmap(source);
    let size = Size::new(SHELF_COVER_WIDTH, SHELF_COVER_HEIGHT).expect("nonzero shelf cover size");
    let mut target =
        PackedImage::new(size, READER_DEPTH, output).expect("exact shelf cover storage");
    for y in 0..SHELF_COVER_HEIGHT {
        for x in 0..SHELF_COVER_WIDTH {
            let source_x = x * 2;
            let source_y = y * 2;
            let sum = u16::from(source.luma(source_x, source_y))
                + u16::from(source.luma(source_x + 1, source_y))
                + u16::from(source.luma(source_x, source_y + 1))
                + u16::from(source.luma(source_x + 1, source_y + 1));
            target.set_luma(x, y, ((sum + 2) / 4) as u8);
        }
    }
}

fn shelf_bitmap(bytes: &[u8]) -> PackedBitmap<'_> {
    PackedBitmap::new(
        Size::new(SHELF_COVER_WIDTH, SHELF_COVER_HEIGHT)
            .expect("the shelf cover dimensions are non-zero"),
        READER_DEPTH,
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
