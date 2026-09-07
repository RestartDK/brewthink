use std::io::Cursor;

use brewthink::{
    app::{
        App, AppEffect, AppInput, AppPreferences, AppView, Direction, FilesState, ImageId,
        ReaderPreferences, ReadingLocation, ResumePoint, SettingsItem, SleepScreenSource,
    },
    epub::{ChapterContent, ContentStyle, EpubBook},
    files::{FileItem, FileKind},
    image::{
        Dither, PackedBitmap, PackedImage, READER_DEPTH, RenderOptions, RgbImage, ScaleMode, Size,
    },
    image_viewer::render_image_viewer,
    input::UsbState,
    library::ShelfBook,
    power::BatteryStatus,
    reader::{ReaderLine, ReaderStyle, ReaderTheme, ReaderView},
    settings::CustomImagePreview,
    sleep::SleepView,
    ui::{AppFrame, render_app},
};
use image::{ImageReader, Limits};
use wasm_bindgen::prelude::*;

const WIDTH: usize = 480;
const HEIGHT: usize = 800;
const COVER_WIDTH: usize = 176;
const COVER_HEIGHT: usize = 264;
const MAX_BOOK_TEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_CHAPTERS: usize = 512;
const PAGE_HEIGHT: usize = brewthink::reader::BODY_BOTTOM - brewthink::reader::BODY_TOP;

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
    pub fn new() -> Self {
        Self::with_preferences(ReaderPreferences::default().packed(), u32::MAX)
    }

    #[wasm_bindgen(js_name = withPreferences)]
    pub fn with_preferences(packed: u32, selected_image: u32) -> Self {
        let preferences = AppPreferences::from_packed(packed).unwrap_or_default();
        let mut books = sample_books();
        for book in &mut books {
            book.layout(preferences.reader());
        }
        let images = sample_images();
        let selected_image = usize::try_from(selected_image)
            .ok()
            .filter(|index| *index < images.len())
            .map(ImageId::new);
        let mut app = App::with_catalog(books.len(), images.len(), selected_image, preferences);
        app.set_battery(BatteryStatus::from_percent(82, UsbState::Disconnected));
        Self { books, images, app }
    }

    #[wasm_bindgen(js_name = fromEpub)]
    pub fn from_epub(
        encoded: &[u8],
        file_name: String,
        packed_preferences: u32,
        selected_image: u32,
    ) -> Result<WebLibrary, JsValue> {
        let preferences = AppPreferences::from_packed(packed_preferences).unwrap_or_default();
        let imported = OwnedBook::from_epub(encoded, &file_name)?;
        let mut books = sample_books();
        books[0] = imported;
        for book in &mut books {
            book.layout(preferences.reader());
        }
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
            AppView::BookCover { book, .. } => {
                let book = &self.books[book.index()];
                let cover = book
                    .cover
                    .as_ref()
                    .ok_or_else(|| JsValue::from_str("cover is unavailable"))?;
                render_app(AppFrame::Cover(cover.frame.bitmap()), &mut frame).map_err(js_error)?;
                FrameMetadata::book("cover", book)
            }
            AppView::Reader(session) => self.render_reader(session.location(), &mut frame)?,
            AppView::ReaderDrawer(drawer) => {
                self.render_reader(drawer.session().location(), &mut frame)?
            }
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

impl Default for WebLibrary {
    fn default() -> Self {
        Self::new()
    }
}

impl WebLibrary {
    fn apply_input(&mut self, input: AppInput) -> bool {
        let previous_preferences = self.app.preferences();
        let effect = self.app.input(input);
        let changed = effect != AppEffect::None;
        if self.app.reader_preferences() != previous_preferences.reader() {
            for book in &mut self.books {
                book.layout(self.app.reader_preferences());
            }
        }
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
                    match self.app.chapter_loaded(
                        self.books[book.index()].chapters.len(),
                        chapter.pages.len(),
                    ) {
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
                AppEffect::Render if matches!(self.app.view(), AppView::BookCover { book, .. } if self.books[book.index()].cover.is_none()) => {
                    self.app.input(AppInput::Confirm)
                }
                AppEffect::None | AppEffect::Render | AppEffect::EnterDeepSleep { .. } => {
                    return changed;
                }
            };
        }
    }

    fn render_library(&self, target: &mut PackedImage<'_>) -> Result<FrameMetadata, JsValue> {
        let books = self
            .books
            .iter()
            .map(|book| {
                ShelfBook::new(
                    &book.title,
                    &book.creator,
                    book.cover.as_ref().map(|cover| cover.thumbnail.bitmap()),
                )
            })
            .collect::<Vec<_>>();
        let state = self.app.library();
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
        render_app(AppFrame::Sleep(SleepView::custom(image.bitmap())), target).map_err(js_error)?;
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
        let page = &chapter.pages[location.page_index()];
        let lines = page
            .lines
            .iter()
            .map(|line| ReaderLine::new(&line.text, line.style))
            .collect::<Vec<_>>();
        let chapter_title = match self.app.view() {
            AppView::ReaderDrawer(drawer) => &book.chapters[drawer.chapter()].title,
            _ => &chapter.title,
        };
        let mut view = ReaderView::new(
            &book.title,
            chapter_title,
            &lines,
            self.app.reader_preferences(),
            self.app.battery(),
        );
        let mut metadata = FrameMetadata {
            screen: "reader",
            title: book.title.clone(),
            creator: book.creator.clone(),
            selected: location.book().index(),
            item_count: self.books.len(),
            page: location.page_index(),
            page_count: location.page_count(),
            chapter: location.spine_index(),
            chapter_count: location.spine_count(),
        };
        if let AppView::ReaderDrawer(drawer) = self.app.view() {
            view = view.with_drawer(drawer);
            metadata.screen = "reader-drawer";
            metadata.creator = match drawer.selected() {
                brewthink::app::ReaderControl::Chapter => format!("Chapter: {chapter_title}"),
                brewthink::app::ReaderControl::Position => {
                    format!("Book position: {}%", drawer.position().percent())
                }
                other => other.label().into(),
            };
            metadata.chapter = drawer.chapter();
        }
        render_app(AppFrame::Reader(view), target).map_err(js_error)?;
        Ok(metadata)
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
                "Chapter {} · page {} · position saved",
                spine_index + 1,
                page_index + 1
            ),
            ResumePoint::Home { .. } => "Home position saved".into(),
            ResumePoint::Books { .. } => "Books position saved".into(),
            ResumePoint::Files { .. } => "Files position saved".into(),
            ResumePoint::Settings { .. } => "Settings position saved".into(),
            ResumePoint::Image { .. } => "Image position saved".into(),
        };

        for source in self.app.sleep_screen_plan(resume).sources() {
            match source {
                SleepScreenSource::CustomImage(image_id) => {
                    let image = &self.images[image_id.index()];
                    render_app(AppFrame::Sleep(SleepView::custom(image.bitmap())), target)
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
                    let Some(cover) = book.cover.as_ref().map(|cover| cover.frame.bitmap()) else {
                        continue;
                    };
                    render_app(AppFrame::Sleep(SleepView::book_cover(cover)), target)
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

struct OwnedBook {
    file_name: String,
    file_size: u32,
    title: String,
    creator: String,
    cover: Option<OwnedCover>,
    chapters: Vec<OwnedChapter>,
}

impl OwnedBook {
    fn from_epub(encoded: &[u8], file_name: &str) -> Result<Self, JsValue> {
        let mut epub = EpubBook::open(encoded).map_err(js_error)?;
        let title = epub.publication().metadata().title().to_owned();
        let creator = epub
            .publication()
            .metadata()
            .primary_creator()
            .unwrap_or("Unknown author")
            .to_owned();
        let cover = epub
            .read_cover()
            .ok()
            .flatten()
            .and_then(|encoded| decode_cover(&encoded).ok());
        let linear_spine = epub
            .publication()
            .spine()
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.is_linear().then_some(index))
            .collect::<Vec<_>>();
        if linear_spine.len() > MAX_CHAPTERS {
            return Err(JsValue::from_str("EPUB exceeds the 512 chapter limit"));
        }
        let titles = epub.chapter_titles();
        let mut text_bytes = 0usize;
        let mut chapters = Vec::with_capacity(linear_spine.len());
        for spine_index in linear_spine {
            let content = epub.read_spine_document(spine_index).map_err(js_error)?;
            text_bytes = text_bytes.saturating_add(
                content
                    .blocks()
                    .iter()
                    .map(|block| block.text().len())
                    .sum::<usize>(),
            );
            if text_bytes > MAX_BOOK_TEXT_BYTES {
                return Err(JsValue::from_str(
                    "EPUB exceeds the 8 MiB rendered-text limit",
                ));
            }
            let mut chapter = OwnedChapter::from_content(content, chapters.len());
            if let Some(Some(title)) = titles.get(spine_index) {
                chapter.title.clone_from(title);
            }
            chapters.push(chapter);
        }
        if chapters.is_empty() {
            return Err(JsValue::from_str("EPUB has no linear readable chapters"));
        }
        Ok(Self {
            file_name: file_name.into(),
            file_size: encoded.len().min(u32::MAX as usize) as u32,
            title,
            creator,
            cover,
            chapters,
        })
    }

    fn layout(&mut self, preferences: ReaderPreferences) {
        for chapter in &mut self.chapters {
            chapter.layout(preferences);
        }
    }
}

struct OwnedChapter {
    title: String,
    source: Vec<OwnedLine>,
    pages: Vec<OwnedPage>,
}

impl OwnedChapter {
    fn from_content(content: ChapterContent, index: usize) -> Self {
        let title = content
            .title()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Section {}", index + 1));
        let source = content
            .blocks()
            .iter()
            .map(|block| OwnedLine {
                text: block.text().into(),
                style: reader_style(block.style()),
            })
            .collect();
        Self {
            title,
            source,
            pages: Vec::new(),
        }
    }

    fn layout(&mut self, preferences: ReaderPreferences) {
        self.pages = paginate(&self.source, preferences);
    }
}

struct OwnedPage {
    lines: Vec<OwnedLine>,
}

struct OwnedLine {
    text: String,
    style: ReaderStyle,
}

struct OwnedCover {
    thumbnail: OwnedBitmap,
    frame: OwnedBitmap,
}

struct OwnedBitmap {
    size: Size,
    pixels: Vec<u8>,
}

impl OwnedBitmap {
    fn bitmap(&self) -> PackedBitmap<'_> {
        PackedBitmap::new(self.size, READER_DEPTH, &self.pixels)
            .expect("owned bitmap shape was checked when it was packed")
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

fn paginate(source: &[OwnedLine], preferences: ReaderPreferences) -> Vec<OwnedPage> {
    let theme = ReaderTheme::from_preferences(preferences);
    let mut pages = Vec::new();
    let mut lines = Vec::new();
    let mut used_height = 0;
    for block in source {
        let style = block.style;
        for text in wrap_text(&block.text, theme, style) {
            push_line(
                &mut pages,
                &mut lines,
                &mut used_height,
                OwnedLine { text, style },
                theme,
            );
        }
        if !lines.is_empty() {
            push_line(
                &mut pages,
                &mut lines,
                &mut used_height,
                OwnedLine {
                    text: String::new(),
                    style: ReaderStyle::Body,
                },
                theme,
            );
        }
    }
    if !lines.is_empty() {
        pages.push(OwnedPage { lines });
    }
    if pages.is_empty() {
        pages.push(OwnedPage {
            lines: vec![OwnedLine {
                text: "This section contains no readable text.".into(),
                style: ReaderStyle::Body,
            }],
        });
    }
    pages
}

fn push_line(
    pages: &mut Vec<OwnedPage>,
    lines: &mut Vec<OwnedLine>,
    used_height: &mut usize,
    line: OwnedLine,
    theme: ReaderTheme,
) {
    let height = theme.line_height(line.style);
    if (*used_height + height > PAGE_HEIGHT || lines.len() == brewthink::reader::MAX_PAGE_LINES)
        && !lines.is_empty()
    {
        pages.push(OwnedPage {
            lines: std::mem::take(lines),
        });
        *used_height = 0;
    }
    *used_height += height;
    lines.push(line);
}

fn wrap_text(text: &str, theme: ReaderTheme, style: ReaderStyle) -> Vec<String> {
    let line_width = theme.line_width(style);
    let space_width = theme.character_width(style, ' ');
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    for word in text.split_whitespace() {
        let word_width = theme.text_width(style, word);
        if !current.is_empty() && current_width + space_width + word_width > line_width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        if word_width > line_width {
            for character in word.chars() {
                let character_width = theme.character_width(style, character);
                if !current.is_empty() && current_width + character_width > line_width {
                    lines.push(std::mem::take(&mut current));
                    current_width = 0;
                }
                current.push(character);
                current_width += character_width;
            }
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
            current_width += space_width;
        }
        current.push_str(word);
        current_width += word_width;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

const fn reader_style(style: ContentStyle) -> ReaderStyle {
    match style {
        ContentStyle::Body => ReaderStyle::Body,
        ContentStyle::Heading => ReaderStyle::Heading,
        ContentStyle::Quote => ReaderStyle::Quote,
        ContentStyle::ListItem => ReaderStyle::ListItem,
        ContentStyle::Preformatted => ReaderStyle::Preformatted,
        ContentStyle::Caption => ReaderStyle::Caption,
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

fn sample_books() -> Vec<OwnedBook> {
    [
        (
            "study-in-scarlet.epub",
            "A Study in Scarlet",
            "Arthur Conan Doyle",
        ),
        (
            "pride-and-prejudice.epub",
            "Pride and Prejudice",
            "Jane Austen",
        ),
        ("walden.epub", "Walden", "Henry David Thoreau"),
        ("frankenstein.epub", "Frankenstein", "Mary Shelley"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (file_name, title, creator))| OwnedBook {
        file_name: file_name.into(),
        file_size: 180_000 + index as u32 * 74_000,
        title: title.into(),
        creator: creator.into(),
        cover: Some(pattern_cover(index)),
        chapters: sample_chapters(title),
    })
    .collect()
}

fn sample_chapters(title: &str) -> Vec<OwnedChapter> {
    (0..3)
        .map(|chapter| {
            let source = (0..18)
                .map(|paragraph| OwnedLine {
                    text: format!(
                        "{} · section {} · passage {}. This public-domain sample proves page turning, chapter boundaries, sleep, wake, and reading-position resume in the shared application state.",
                        title,
                        chapter + 1,
                        paragraph + 1
                    ),
                    style: if paragraph == 0 {
                        ReaderStyle::Heading
                    } else {
                        ReaderStyle::Body
                    },
                })
                .collect();
            OwnedChapter {
                title: format!("Section {}", chapter + 1),
                source,
                pages: Vec::new(),
            }
        })
        .collect()
}

fn decode_cover(encoded: &[u8]) -> Result<OwnedCover, JsValue> {
    let mut reader = ImageReader::new(Cursor::new(encoded))
        .with_guessed_format()
        .map_err(js_error)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(2_048);
    limits.max_image_height = Some(2_048);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let rgba = reader.decode().map_err(js_error)?.into_rgba8();
    let size = Size::new(rgba.width() as usize, rgba.height() as usize).map_err(js_error)?;
    let mut pixels = Vec::with_capacity(size.width() * size.height() * 3);
    for pixel in rgba.pixels() {
        let alpha = u16::from(pixel[3]);
        for channel in &pixel.0[..3] {
            let composited = (u16::from(*channel) * alpha + 255 * (255 - alpha) + 127) / 255;
            pixels.push(composited as u8);
        }
    }
    let source = RgbImage::new(size, &pixels).map_err(js_error)?;
    Ok(pack_cover(&source))
}

fn pattern_cover(index: usize) -> OwnedCover {
    const WIDTH: usize = 48;
    const HEIGHT: usize = 72;
    let mut pixels = vec![255; WIDTH * HEIGHT * 3];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let border = x < 2 || y < 2 || x >= WIDTH - 2 || y >= HEIGHT - 2;
            let pattern = match index {
                0 => (x / 6 + y / 6) % 2 == 0,
                1 => x % 11 < 3,
                2 => (x + y) % 13 < 4,
                _ => x.abs_diff(WIDTH / 2) + y.abs_diff(HEIGHT / 2) < 18,
            };
            let shade = if border || pattern { 24 } else { 232 };
            let offset = (y * WIDTH + x) * 3;
            pixels[offset..offset + 3].fill(shade);
        }
    }
    let source = RgbImage::new(Size::new(WIDTH, HEIGHT).unwrap(), &pixels).unwrap();
    pack_cover(&source)
}

fn pack_cover(source: &RgbImage<'_>) -> OwnedCover {
    OwnedCover {
        thumbnail: pack_bitmap(
            source,
            Size::new(COVER_WIDTH, COVER_HEIGHT).unwrap(),
            ScaleMode::Cover,
        ),
        frame: pack_bitmap(
            source,
            Size::new(WIDTH, HEIGHT).unwrap(),
            ScaleMode::Contain,
        ),
    }
}

fn pack_bitmap(source: &RgbImage<'_>, size: Size, scale: ScaleMode) -> OwnedBitmap {
    let mut pixels = vec![0xFF; READER_DEPTH.byte_len(size).unwrap()];
    let mut target = PackedImage::new(size, READER_DEPTH, &mut pixels).unwrap();
    brewthink::image::render(
        source,
        &mut target,
        RenderOptions {
            scale,
            dither: Dither::None,
        },
    );
    OwnedBitmap { size, pixels }
}

#[wasm_bindgen]
pub fn front_button_centers() -> Vec<i32> {
    brewthink::ui::FRONT_BUTTON_CENTERS.to_vec()
}

#[wasm_bindgen]
pub fn renderer_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

fn js_error(error: impl core::fmt::Debug) -> JsValue {
    JsValue::from_str(&format!("{error:?}"))
}

fn main() {}
