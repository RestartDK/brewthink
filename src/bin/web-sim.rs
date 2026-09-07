use brewthink::{
    app::{
        App, AppEffect, AppInput, AppPreferences, AppView, Direction, FilesState, ImageId,
        ReaderPreferences, ReadingLocation, ResumePoint, SettingsItem, SleepScreenSource,
    },
    cover::{SHELF_COVER_BYTES, downsample_cover, shelf_bitmap},
    files::{FileItem, FileKind},
    image::{PackedBitmap, PackedImage, READER_DEPTH, Size},
    image_viewer::render_image_viewer,
    input::UsbState,
    library::ShelfBook,
    power::BatteryStatus,
    reader::{ReaderLine, ReaderView},
    settings::CustomImagePreview,
    simulator::{Book as OwnedBook, Cover, sample_books},
    sleep::SleepView,
    ui::{AppFrame, render_app},
};
use wasm_bindgen::prelude::*;

const WIDTH: usize = 480;
const HEIGHT: usize = 800;

#[wasm_bindgen]
#[derive(Clone, Copy)]
pub enum ShelfDirection {
    Left,
    Right,
    Up,
    Down,
}

#[wasm_bindgen]
#[derive(Clone, Copy)]
pub enum WebInput {
    Left,
    Right,
    Up,
    Down,
    Confirm,
    Back,
    Power,
}

#[wasm_bindgen]
pub struct RenderedFrame {
    pixels: Vec<u8>,
    screen: &'static str,
    title: String,
    creator: String,
    selected: usize,
    item_count: usize,
    page: usize,
    page_count: usize,
    chapter: usize,
    chapter_count: usize,
}

#[wasm_bindgen]
impl RenderedFrame {
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> usize {
        WIDTH
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> usize {
        HEIGHT
    }

    #[wasm_bindgen(getter)]
    pub fn screen(&self) -> String {
        self.screen.into()
    }

    #[wasm_bindgen(getter)]
    pub fn title(&self) -> String {
        self.title.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn creator(&self) -> String {
        self.creator.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn selected(&self) -> usize {
        self.selected
    }

    #[wasm_bindgen(getter)]
    pub fn item_count(&self) -> usize {
        self.item_count
    }

    #[wasm_bindgen(getter)]
    pub fn page(&self) -> usize {
        self.page
    }

    #[wasm_bindgen(getter)]
    pub fn page_count(&self) -> usize {
        self.page_count
    }

    #[wasm_bindgen(getter)]
    pub fn chapter(&self) -> usize {
        self.chapter
    }

    #[wasm_bindgen(getter)]
    pub fn chapter_count(&self) -> usize {
        self.chapter_count
    }

    #[wasm_bindgen(getter)]
    pub fn payload_bytes(&self) -> usize {
        self.pixels.len()
    }

    #[wasm_bindgen(getter)]
    pub fn bits_per_pixel(&self) -> usize {
        READER_DEPTH.bits()
    }

    pub fn pixels(&self) -> Vec<u8> {
        self.pixels.clone()
    }
}

#[wasm_bindgen]
pub struct WebLibrary {
    books: Vec<OwnedBook>,
    images: Vec<OwnedImage>,
    app: App,
}

#[wasm_bindgen]
impl WebLibrary {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Self, JsValue> {
        Self::with_preferences(ReaderPreferences::default().packed(), u32::MAX)
    }

    #[wasm_bindgen(js_name = withPreferences)]
    pub fn with_preferences(packed: u32, selected_image: u32) -> Result<Self, JsValue> {
        let preferences = AppPreferences::from_packed(packed).unwrap_or_default();
        let books = sample_books().map_err(js_error)?;
        let images = sample_images();
        let selected_image = usize::try_from(selected_image)
            .ok()
            .filter(|index| *index < images.len())
            .map(ImageId::new);
        let mut app = App::with_catalog(books.len(), images.len(), selected_image, preferences);
        app.set_battery(BatteryStatus::from_percent(82, UsbState::Disconnected));
        Ok(Self { books, images, app })
    }

    #[wasm_bindgen(js_name = fromEpub)]
    pub fn from_epub(
        encoded: &[u8],
        file_name: String,
        packed_preferences: u32,
        selected_image: u32,
    ) -> Result<WebLibrary, JsValue> {
        let preferences = AppPreferences::from_packed(packed_preferences).unwrap_or_default();
        let imported = OwnedBook::from_epub(encoded, &file_name).map_err(js_error)?;
        let mut books = sample_books().map_err(js_error)?;
        books[0] = imported;
        let images = sample_images();
        let selected_image = usize::try_from(selected_image)
            .ok()
            .filter(|index| *index < images.len())
            .map(ImageId::new);
        let mut app = App::with_catalog(books.len(), images.len(), selected_image, preferences);
        app.set_battery(BatteryStatus::from_percent(82, UsbState::Disconnected));
        Ok(Self { books, images, app })
    }

    #[wasm_bindgen(getter)]
    pub fn preferences(&self) -> u32 {
        self.app.preferences().packed()
    }

    #[wasm_bindgen(getter, js_name = selectedImage)]
    pub fn selected_image(&self) -> u32 {
        self.app
            .selected_sleep_image()
            .and_then(|image| u32::try_from(image.index()).ok())
            .unwrap_or(u32::MAX)
    }

    pub fn move_selection(&mut self, direction: ShelfDirection) -> bool {
        self.apply_input(AppInput::Move(match direction {
            ShelfDirection::Left => Direction::Left,
            ShelfDirection::Right => Direction::Right,
            ShelfDirection::Up => Direction::Up,
            ShelfDirection::Down => Direction::Down,
        }))
    }

    pub fn input(&mut self, input: WebInput) -> bool {
        let input = match input {
            WebInput::Left => AppInput::Move(Direction::Left),
            WebInput::Right => AppInput::Move(Direction::Right),
            WebInput::Up => AppInput::Move(Direction::Up),
            WebInput::Down => AppInput::Move(Direction::Down),
            WebInput::Confirm => AppInput::Confirm,
            WebInput::Back => AppInput::Back,
            WebInput::Power => AppInput::Power,
        };
        self.apply_input(input)
    }

    pub fn wake(&mut self) -> bool {
        let effect = self.app.wake();
        self.resolve_effect(effect)
    }

    pub fn render(&self) -> Result<RenderedFrame, JsValue> {
        let size = Size::new(WIDTH, HEIGHT).unwrap();
        let mut pixels = vec![0xFF; READER_DEPTH.byte_len(size).unwrap()];
        let mut frame = PackedImage::new(size, READER_DEPTH, &mut pixels).map_err(js_error)?;
        let metadata = match self.app.view() {
            AppView::Home(state) => {
                render_app(
                    AppFrame::Home {
                        state,
                        battery: self.app.battery(),
                    },
                    &mut frame,
                )
                .map_err(js_error)?;
                FrameMetadata::selection(
                    "home",
                    state.selected().label(),
                    "Primary menu",
                    state.selected().index(),
                    3,
                )
            }
            AppView::Library => self.render_library(&mut frame)?,
            AppView::Files(state) => self.render_files(state, &mut frame)?,
            AppView::Settings(state) => {
                let selected = self
                    .app
                    .selected_sleep_image()
                    .and_then(|image| self.images.get(image.index()));
                render_app(
                    AppFrame::Settings {
                        state,
                        battery: self.app.battery(),
                        custom_image: selected.map_or(CustomImagePreview::Missing, |image| {
                            CustomImagePreview::Ready {
                                name: &image.name,
                                bitmap: image.bitmap(),
                            }
                        }),
                    },
                    &mut frame,
                )
                .map_err(js_error)?;
                FrameMetadata::selection(
                    "settings",
                    state.selected().label(),
                    state
                        .selected()
                        .value(state.draft())
                        .unwrap_or("Confirm to save"),
                    state.selected().index(),
                    SettingsItem::ALL.len(),
                )
            }
            AppView::Reader(session) => self.render_reader(session.location(), &mut frame)?,
            AppView::Image(image) => self.render_image(image, &mut frame)?,
            AppView::Sleeping { resume } => self.render_sleep(resume, &mut frame)?,
            AppView::Error { book, .. } => {
                let book = &self.books[book.index()];
                render_app(
                    AppFrame::Error {
                        book_title: &book.title,
                        message: "This EPUB or chapter could not be opened.",
                        battery: self.app.battery(),
                    },
                    &mut frame,
                )
                .map_err(js_error)?;
                FrameMetadata::book("error", book)
            }
            AppView::Loading(_) => return Err(JsValue::from_str("chapter load did not resolve")),
        };
        Ok(RenderedFrame {
            pixels,
            screen: metadata.screen,
            title: metadata.title,
            creator: metadata.creator,
            selected: metadata.selected,
            item_count: metadata.item_count,
            page: metadata.page,
            page_count: metadata.page_count,
            chapter: metadata.chapter,
            chapter_count: metadata.chapter_count,
        })
    }
}

impl WebLibrary {
    fn apply_input(&mut self, input: AppInput) -> bool {
        let effect = self.app.input(input);
        let changed = effect != AppEffect::None;
        self.resolve_effect(effect);
        changed
    }

    fn resolve_effect(&mut self, mut effect: AppEffect) -> bool {
        let changed = effect != AppEffect::None;
        loop {
            effect = match effect {
                AppEffect::LoadChapter {
                    book, spine_index, ..
                } => {
                    let Some(chapter) = self
                        .books
                        .get(book.index())
                        .and_then(|book| book.chapters.get(spine_index))
                    else {
                        return self.app.chapter_failed().is_ok();
                    };
                    let page = match chapter.page(0, self.app.reader_preferences()) {
                        Ok(page) => page,
                        Err(_) => return self.app.chapter_failed().is_ok(),
                    };
                    match self
                        .app
                        .chapter_loaded(self.books[book.index()].chapters.len(), page.page_count())
                    {
                        Ok(next) => next,
                        Err(_) => return false,
                    }
                }
                AppEffect::Render if matches!(self.app.view(), AppView::Sleeping { .. }) => {
                    match self.app.sleep_frame_ready() {
                        Ok(next) => next,
                        Err(_) => return false,
                    }
                }
                AppEffect::None | AppEffect::Render | AppEffect::EnterDeepSleep { .. } => {
                    return changed;
                }
            };
        }
    }

    fn render_library(&self, target: &mut PackedImage<'_>) -> Result<FrameMetadata, JsValue> {
        let state = self.app.library();
        let mut thumbnails = vec![[0xff; SHELF_COVER_BYTES]; self.books.len()];
        let books = self
            .books
            .iter()
            .zip(thumbnails.iter_mut())
            .enumerate()
            .map(|(index, (book, thumbnail))| {
                let cover = match &book.cover {
                    Cover::Decoded(bytes)
                        if state.selected().is_some_and(|id| id.index() != index) =>
                    {
                        downsample_cover(bytes, thumbnail);
                        Some(shelf_bitmap(thumbnail))
                    }
                    cover => cover.bitmap(),
                };
                ShelfBook::new(&book.title, &book.creator, cover)
            })
            .collect::<Vec<_>>();
        render_app(
            AppFrame::Library {
                state,
                books: &books,
                battery: self.app.battery(),
            },
            target,
        )
        .map_err(js_error)?;
        let selected = state.selected().expect("the web catalog is non-empty");
        let book = &self.books[selected.index()];
        Ok(FrameMetadata {
            screen: "library",
            title: book.title.clone(),
            creator: book.creator.clone(),
            selected: selected.index(),
            item_count: books.len(),
            page: state.page(),
            page_count: state.page_count(),
            chapter: 0,
            chapter_count: 0,
        })
    }

    fn render_files(
        &self,
        state: FilesState,
        target: &mut PackedImage<'_>,
    ) -> Result<FrameMetadata, JsValue> {
        let files = self
            .books
            .iter()
            .map(|book| FileItem::new(&book.file_name, book.file_size, FileKind::Epub))
            .chain(
                self.images
                    .iter()
                    .map(|image| FileItem::new(&image.name, image.size, image.kind)),
            )
            .collect::<Vec<_>>();
        render_app(
            AppFrame::Files {
                state,
                files: &files,
                battery: self.app.battery(),
            },
            target,
        )
        .map_err(js_error)?;
        let selected = state.selected().expect("the web catalog is non-empty");
        let (title, creator) = if selected.index() < self.books.len() {
            let book = &self.books[selected.index()];
            (
                book.file_name.clone(),
                format!("{} KiB · EPUB", book.file_size.div_ceil(1024)),
            )
        } else {
            let image = &self.images[selected.index() - self.books.len()];
            (
                image.name.clone(),
                format!("{} KiB · {}", image.size.div_ceil(1024), image.kind.label()),
            )
        };
        Ok(FrameMetadata {
            screen: "files",
            title,
            creator,
            selected: selected.index(),
            item_count: files.len(),
            page: state.page(),
            page_count: state.page_count(),
            chapter: 0,
            chapter_count: 0,
        })
    }

    fn render_image(
        &self,
        image: ImageId,
        target: &mut PackedImage<'_>,
    ) -> Result<FrameMetadata, JsValue> {
        let image = &self.images[image.index()];
        render_app(
            AppFrame::Sleep(SleepView::custom(image.bitmap(), self.app.battery())),
            target,
        )
        .map_err(js_error)?;
        render_image_viewer(
            &image.name,
            self.app.selected_sleep_image() == Some(ImageId::new(image.index)),
            self.app.battery(),
            target,
        )
        .map_err(js_error)?;
        Ok(FrameMetadata::selection(
            "image",
            &image.name,
            if self.app.selected_sleep_image() == Some(ImageId::new(image.index)) {
                "Selected for sleep"
            } else {
                "Confirm to select for sleep"
            },
            image.index,
            self.images.len(),
        ))
    }

    fn render_reader(
        &self,
        location: ReadingLocation,
        target: &mut PackedImage<'_>,
    ) -> Result<FrameMetadata, JsValue> {
        let book = &self.books[location.book().index()];
        let chapter = &book.chapters[location.spine_index()];
        let page = chapter
            .page(location.page_index(), self.app.reader_preferences())
            .map_err(js_error)?;
        let lines = page
            .lines()
            .map(|line| ReaderLine::new(line.text(), line.style()))
            .collect::<Vec<_>>();
        render_app(
            AppFrame::Reader(ReaderView::new(
                &book.title,
                page.chapter_title(),
                &lines,
                location,
                self.app.reader_preferences(),
                self.app.battery(),
            )),
            target,
        )
        .map_err(js_error)?;
        Ok(FrameMetadata {
            screen: "reader",
            title: book.title.clone(),
            creator: book.creator.clone(),
            selected: location.book().index(),
            item_count: self.books.len(),
            page: location.page_index(),
            page_count: location.page_count(),
            chapter: location.spine_index(),
            chapter_count: location.spine_count(),
        })
    }

    fn render_sleep(
        &self,
        resume: ResumePoint,
        target: &mut PackedImage<'_>,
    ) -> Result<FrameMetadata, JsValue> {
        let status = match resume {
            ResumePoint::Reader {
                spine_index,
                page_index,
                ..
            } => format!(
                "CHAPTER {} · PAGE {} · POSITION SAVED",
                spine_index + 1,
                page_index + 1
            ),
            ResumePoint::Home { .. } => "HOME POSITION SAVED".into(),
            ResumePoint::Books { .. } => "BOOKS POSITION SAVED".into(),
            ResumePoint::Files { .. } => "FILES POSITION SAVED".into(),
            ResumePoint::Settings { .. } => "SETTINGS POSITION SAVED".into(),
            ResumePoint::Image { .. } => "IMAGE POSITION SAVED".into(),
        };

        for source in self.app.sleep_screen_plan(resume).sources() {
            match source {
                SleepScreenSource::CustomImage(image_id) => {
                    let image = &self.images[image_id.index()];
                    render_app(
                        AppFrame::Sleep(SleepView::custom(image.bitmap(), self.app.battery())),
                        target,
                    )
                    .map_err(js_error)?;
                    return Ok(FrameMetadata::selection(
                        "sleep",
                        &image.name,
                        "Selected custom sleep image",
                        image_id.index(),
                        self.images.len(),
                    ));
                }
                SleepScreenSource::BookCover(book_id) => {
                    let book = &self.books[book_id.index()];
                    let Some(cover) = book.cover.bitmap() else {
                        continue;
                    };
                    render_app(
                        AppFrame::Sleep(SleepView::book_cover(
                            &book.title,
                            &book.creator,
                            &status,
                            cover,
                            self.app.battery(),
                        )),
                        target,
                    )
                    .map_err(js_error)?;
                    let mut metadata = FrameMetadata::book("sleep", book);
                    metadata.selected = book_id.index();
                    return Ok(metadata);
                }
                SleepScreenSource::BuiltIn => {
                    render_app(
                        AppFrame::Sleep(SleepView::built_in(&status, self.app.battery())),
                        target,
                    )
                    .map_err(js_error)?;
                    return Ok(FrameMetadata::selection(
                        "sleep",
                        "Brewthink",
                        "Built-in fallback",
                        0,
                        0,
                    ));
                }
            }
        }
        Err(JsValue::from_str("sleep plan has no renderable source"))
    }
}

struct FrameMetadata {
    screen: &'static str,
    title: String,
    creator: String,
    selected: usize,
    item_count: usize,
    page: usize,
    page_count: usize,
    chapter: usize,
    chapter_count: usize,
}

impl FrameMetadata {
    fn selection(
        screen: &'static str,
        title: &str,
        creator: &str,
        selected: usize,
        item_count: usize,
    ) -> Self {
        Self {
            screen,
            title: title.into(),
            creator: creator.into(),
            selected,
            item_count,
            page: 0,
            page_count: 0,
            chapter: 0,
            chapter_count: 0,
        }
    }

    fn book(screen: &'static str, book: &OwnedBook) -> Self {
        Self {
            screen,
            title: book.title.clone(),
            creator: book.creator.clone(),
            selected: 0,
            item_count: 0,
            page: 0,
            page_count: 0,
            chapter: 0,
            chapter_count: 0,
        }
    }
}

struct OwnedImage {
    index: usize,
    name: String,
    size: u32,
    kind: FileKind,
    pixels: Vec<u8>,
}

impl OwnedImage {
    fn sample(index: usize, name: &str, kind: FileKind) -> Self {
        let size = Size::new(WIDTH, HEIGHT).unwrap();
        let mut pixels = vec![0xFF; READER_DEPTH.byte_len(size).unwrap()];
        let mut image = PackedImage::new(size, READER_DEPTH, &mut pixels).unwrap();
        let step = 22 + index * 7;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let border = !(20..WIDTH - 20).contains(&x) || !(20..HEIGHT - 20).contains(&y);
                let diagonal = (x / step + y / step + index).is_multiple_of(5);
                let shade = if border || diagonal {
                    0
                } else {
                    (x * 255 / (WIDTH - 1)) as u8
                };
                image.set_luma(x, y, shade);
            }
        }
        Self {
            index,
            name: name.into(),
            size: 48_000,
            kind,
            pixels,
        }
    }

    fn bitmap(&self) -> PackedBitmap<'_> {
        PackedBitmap::new(
            Size::new(WIDTH, HEIGHT).unwrap(),
            READER_DEPTH,
            &self.pixels,
        )
        .expect("sample image has the exact frame shape")
    }
}

fn sample_images() -> Vec<OwnedImage> {
    [
        ("ANOTHE.JPG", FileKind::Jpeg),
        ("AYA.JPG", FileKind::Jpeg),
        ("MOOD.JPG", FileKind::Jpeg),
        ("NICE.JPG", FileKind::Jpeg),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (name, kind))| OwnedImage::sample(index, name, kind))
    .collect()
}

#[wasm_bindgen]
pub fn renderer_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

fn js_error(error: impl core::fmt::Debug) -> JsValue {
    JsValue::from_str(&format!("{error:?}"))
}

fn main() {}
