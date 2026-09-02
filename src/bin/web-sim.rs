use std::io::Cursor;

use brewthink::{
    app::{App, AppEffect, AppInput, AppView, Direction, ReadingLocation, ResumePoint},
    epub::{ChapterContent, ContentStyle, EpubBook},
    image::{Dither, MonochromeBitmap, MonochromeImage, RenderOptions, RgbImage, ScaleMode, Size},
    library::{ShelfBook, render_shelf},
    reader::{ReaderLine, ReaderStyle, ReaderView, render_reader},
    sleep::{SleepView, render_sleep},
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

    pub fn pixels(&self) -> Vec<u8> {
        self.pixels.clone()
    }
}

#[wasm_bindgen]
pub struct WebLibrary {
    books: Vec<OwnedBook>,
    app: App,
}

#[wasm_bindgen]
impl WebLibrary {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let books = sample_books();
        let app = App::new(books.len());
        Self { books, app }
    }

    #[wasm_bindgen(js_name = fromEpub)]
    pub fn from_epub(encoded: &[u8]) -> Result<WebLibrary, JsValue> {
        let imported = OwnedBook::from_epub(encoded)?;
        let mut books = sample_books();
        books[0] = imported;
        let app = App::new(books.len());
        Ok(Self { books, app })
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
        let mut pixels = vec![0xFF; WIDTH * HEIGHT / 8];
        let mut frame = MonochromeImage::new(Size::new(WIDTH, HEIGHT).unwrap(), &mut pixels)
            .map_err(js_error)?;
        let metadata = match self.app.view() {
            AppView::Library => self.render_library(&mut frame)?,
            AppView::Reader(location) => self.render_reader(location, &mut frame)?,
            AppView::Sleeping { resume } => self.render_sleep(resume, &mut frame)?,
            AppView::Error { book } => {
                let book = &self.books[book.index()];
                render_sleep(
                    SleepView::new(
                        &book.title,
                        &book.creator,
                        "BOOK ERROR · BACK TO LIBRARY",
                        book.cover.as_ref().map(OwnedCover::bitmap),
                    ),
                    &mut frame,
                )
                .map_err(js_error)?;
                FrameMetadata::book("error", book)
            }
            AppView::Loading => return Err(JsValue::from_str("chapter load did not resolve")),
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
        let effect = self.app.input(input);
        let changed = effect != AppEffect::None;
        self.resolve_effect(effect);
        changed
    }

    fn resolve_effect(&mut self, mut effect: AppEffect) -> bool {
        let mut changed = effect != AppEffect::None;
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
                AppEffect::RenderSleep { .. } => {
                    changed = true;
                    match self.app.sleep_frame_ready() {
                        Ok(next) => next,
                        Err(_) => return false,
                    }
                }
                AppEffect::None
                | AppEffect::RenderLibrary
                | AppEffect::RenderReader(_)
                | AppEffect::RenderError { .. }
                | AppEffect::EnterDeepSleep { .. } => return changed,
            };
        }
    }

    fn render_library(&self, target: &mut MonochromeImage<'_>) -> Result<FrameMetadata, JsValue> {
        let books = self
            .books
            .iter()
            .map(|book| {
                ShelfBook::new(
                    &book.title,
                    &book.creator,
                    book.cover.as_ref().map(OwnedCover::bitmap),
                )
            })
            .collect::<Vec<_>>();
        let state = self.app.library();
        render_shelf(state, &books, target).map_err(js_error)?;
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

    fn render_reader(
        &self,
        location: ReadingLocation,
        target: &mut MonochromeImage<'_>,
    ) -> Result<FrameMetadata, JsValue> {
        let book = &self.books[location.book().index()];
        let chapter = &book.chapters[location.spine_index()];
        let page = &chapter.pages[location.page_index()];
        let lines = page
            .lines
            .iter()
            .map(|line| ReaderLine::new(&line.text, line.style))
            .collect::<Vec<_>>();
        render_reader(
            ReaderView::new(&book.title, &chapter.title, &lines, location),
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
        target: &mut MonochromeImage<'_>,
    ) -> Result<FrameMetadata, JsValue> {
        let selected = match resume {
            ResumePoint::Reader { book, .. } => Some(book.index()),
            ResumePoint::Library { selected } => selected.map(|book| book.index()),
        }
        .unwrap_or(0);
        let book = &self.books[selected];
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
            ResumePoint::Library { .. } => "LIBRARY POSITION SAVED".into(),
        };
        render_sleep(
            SleepView::new(
                &book.title,
                &book.creator,
                &status,
                book.cover.as_ref().map(OwnedCover::bitmap),
            ),
            target,
        )
        .map_err(js_error)?;
        let mut metadata = FrameMetadata::book("sleep", book);
        metadata.selected = selected;
        Ok(metadata)
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
    title: String,
    creator: String,
    cover: Option<OwnedCover>,
    chapters: Vec<OwnedChapter>,
}

impl OwnedBook {
    fn from_epub(encoded: &[u8]) -> Result<Self, JsValue> {
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
            .map_err(js_error)?
            .map(|encoded| decode_cover(&encoded))
            .transpose()?;
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
            chapters.push(OwnedChapter::from_content(content, chapters.len()));
        }
        if chapters.is_empty() {
            return Err(JsValue::from_str("EPUB has no linear readable chapters"));
        }
        Ok(Self {
            title,
            creator,
            cover,
            chapters,
        })
    }
}

struct OwnedChapter {
    title: String,
    pages: Vec<OwnedPage>,
}

impl OwnedChapter {
    fn from_content(content: ChapterContent, index: usize) -> Self {
        let title = content
            .title()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Section {}", index + 1));
        let pages = paginate(&content);
        Self { title, pages }
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
    size: Size,
    pixels: Vec<u8>,
}

impl OwnedCover {
    fn bitmap(&self) -> MonochromeBitmap<'_> {
        MonochromeBitmap::new(self.size, &self.pixels)
            .expect("owned cover shape was checked when it was packed")
    }
}

fn paginate(content: &ChapterContent) -> Vec<OwnedPage> {
    let mut pages = Vec::new();
    let mut lines = Vec::new();
    let mut used_height = 0;
    for block in content.blocks() {
        let style = reader_style(block.style());
        for text in wrap_text(block.text(), style.characters_per_line()) {
            push_line(
                &mut pages,
                &mut lines,
                &mut used_height,
                OwnedLine { text, style },
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
) {
    let height = line.style.line_height();
    if *used_height + height > PAGE_HEIGHT && !lines.is_empty() {
        pages.push(OwnedPage {
            lines: std::mem::take(lines),
        });
        *used_height = 0;
    }
    *used_height += height;
    lines.push(line);
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if word.chars().count() > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let mut remainder = word;
            while remainder.chars().count() > width {
                let split = remainder
                    .char_indices()
                    .nth(width)
                    .map(|(index, _)| index)
                    .unwrap_or(remainder.len());
                lines.push(remainder[..split].to_owned());
                remainder = &remainder[split..];
            }
            current.push_str(remainder);
            continue;
        }
        let separator = usize::from(!current.is_empty());
        if current.chars().count() + separator + word.chars().count() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
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

fn sample_books() -> Vec<OwnedBook> {
    [
        ("A Study in Scarlet", "Arthur Conan Doyle"),
        ("Pride and Prejudice", "Jane Austen"),
        ("Walden", "Henry David Thoreau"),
        ("Frankenstein", "Mary Shelley"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (title, creator))| OwnedBook {
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
            let mut lines = Vec::new();
            for paragraph in 0..18 {
                lines.push(OwnedLine {
                    text: format!(
                        "{} · section {} · passage {}. This public-domain sample proves page turning, chapter boundaries, sleep, wake, and exact reading-position resume in the shared application state.",
                        title,
                        chapter + 1,
                        paragraph + 1
                    ),
                    style: if paragraph == 0 {
                        ReaderStyle::Heading
                    } else {
                        ReaderStyle::Body
                    },
                });
                lines.push(OwnedLine {
                    text: String::new(),
                    style: ReaderStyle::Body,
                });
            }
            let content = lines
                .into_iter()
                .flat_map(|line| {
                    wrap_text(&line.text, line.style.characters_per_line())
                        .into_iter()
                        .map(move |text| OwnedLine {
                            text,
                            style: line.style,
                        })
                })
                .collect::<Vec<_>>();
            let mut pages = Vec::new();
            let mut page = Vec::new();
            let mut used = 0;
            for line in content {
                push_line(&mut pages, &mut page, &mut used, line);
            }
            if !page.is_empty() {
                pages.push(OwnedPage { lines: page });
            }
            OwnedChapter {
                title: format!("Section {}", chapter + 1),
                pages,
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
    let size = Size::new(COVER_WIDTH, COVER_HEIGHT).unwrap();
    let mut pixels = vec![0xFF; COVER_WIDTH * COVER_HEIGHT / 8];
    let mut target = MonochromeImage::new(size, &mut pixels).unwrap();
    brewthink::image::render(
        source,
        &mut target,
        RenderOptions {
            scale: ScaleMode::Cover,
            dither: Dither::Ordered4x4,
        },
    );
    OwnedCover { size, pixels }
}

#[wasm_bindgen]
pub fn renderer_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

fn js_error(error: impl core::fmt::Debug) -> JsValue {
    JsValue::from_str(&format!("{error:?}"))
}

fn main() {}
