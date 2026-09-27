use crate::{
    app::ReaderPreferences,
    bounded_xml::{FixedString, XmlError, XmlEvent, XmlReader, XmlText},
    image::ScaleMode,
    image_cache::{ImageResource, ImageSpec},
    reader::{ReaderStyle, ReaderTheme},
};

mod record;
pub use record::MAX_ENCODED_PAGE_BYTES;

pub use crate::reader::MAX_PAGE_LINES;
pub const MAX_READER_LINE_BYTES: usize = 320;
pub const MAX_CHAPTER_TITLE_BYTES: usize = 96;
const PAGE_HEIGHT: usize = crate::reader::BODY_BOTTOM - crate::reader::BODY_TOP;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedReaderLine {
    text: FixedString<MAX_READER_LINE_BYTES>,
    style: ReaderStyle,
    top: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedImage {
    resource: ImageResource,
    spec: ImageSpec,
    top: u16,
    alt: FixedString<128>,
}

impl BoundedImage {
    pub fn path(&self) -> &str {
        self.resource.path.as_str()
    }
    pub const fn resource(&self) -> &ImageResource {
        &self.resource
    }
    pub const fn spec(&self) -> ImageSpec {
        self.spec
    }
    pub const fn top(&self) -> usize {
        self.top as usize
    }
    pub fn alt(&self) -> &str {
        self.alt.as_str()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PageElement {
    Text(BoundedReaderLine),
    Image(BoundedImage),
}

impl BoundedReaderLine {
    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    pub const fn style(&self) -> ReaderStyle {
        self.style
    }

    pub const fn top(&self) -> usize {
        self.top as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedPage {
    lines: [Option<PageElement>; MAX_PAGE_LINES],
    line_count: u8,
    page_index: usize,
    page_count: usize,
    chapter_title: FixedString<MAX_CHAPTER_TITLE_BYTES>,
}

impl Default for BoundedPage {
    fn default() -> Self {
        Self::new()
    }
}

impl BoundedPage {
    pub const fn new() -> Self {
        Self {
            lines: [None; MAX_PAGE_LINES],
            line_count: 0,
            page_index: 0,
            page_count: 0,
            chapter_title: FixedString::new(),
        }
    }

    #[cfg(any(target_arch = "riscv32", test))]
    pub(crate) unsafe fn initialize_in_place(page: *mut Self) {
        // SAFETY: the caller provides writable aligned storage; every field is initialized.
        unsafe {
            let lines = core::ptr::addr_of_mut!((*page).lines).cast::<Option<PageElement>>();
            for index in 0..MAX_PAGE_LINES {
                lines.add(index).write(None);
            }
            core::ptr::addr_of_mut!((*page).line_count).write(0);
            core::ptr::addr_of_mut!((*page).page_index).write(0);
            core::ptr::addr_of_mut!((*page).page_count).write(0);
            core::ptr::addr_of_mut!((*page).chapter_title).write(FixedString::new());
        }
    }

    fn reset(&mut self, requested_page: usize) {
        for line in &mut self.lines {
            *line = None;
        }
        self.line_count = 0;
        self.page_index = requested_page;
        self.page_count = 0;
        self.chapter_title.clear();
    }

    pub fn lines(&self) -> impl Iterator<Item = &BoundedReaderLine> {
        self.lines[..usize::from(self.line_count)]
            .iter()
            .flatten()
            .filter_map(|element| match element {
                PageElement::Text(line) => Some(line),
                PageElement::Image(_) => None,
            })
    }

    pub fn images(&self) -> impl Iterator<Item = &BoundedImage> {
        self.lines[..usize::from(self.line_count)]
            .iter()
            .flatten()
            .filter_map(|element| match element {
                PageElement::Image(image) => Some(image),
                PageElement::Text(_) => None,
            })
    }

    pub const fn page_index(&self) -> usize {
        self.page_index
    }

    pub const fn page_count(&self) -> usize {
        self.page_count
    }

    pub fn chapter_title(&self) -> &str {
        self.chapter_title.as_str()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutError {
    Xml(XmlError),
    PageOutOfBounds,
    TooManyLines,
    ChapterCapacity,
    InvalidPageRecord,
}

impl core::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Xml(error) => write!(f, "{error}"),
            Self::PageOutOfBounds => f.write_str("reader page is out of bounds"),
            Self::TooManyLines => f.write_str("reader page exceeds line capacity"),
            Self::ChapterCapacity => f.write_str("chapter exceeds page cache capacity"),
            Self::InvalidPageRecord => f.write_str("chapter page record is invalid"),
        }
    }
}

impl core::error::Error for LayoutError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Xml(error) => Some(error),
            Self::PageOutOfBounds
            | Self::TooManyLines
            | Self::ChapterCapacity
            | Self::InvalidPageRecord => None,
        }
    }
}

impl From<XmlError> for LayoutError {
    fn from(error: XmlError) -> Self {
        Self::Xml(error)
    }
}

pub fn layout_xhtml_page(
    encoded: &[u8],
    requested_page: usize,
    preferences: ReaderPreferences,
) -> Result<BoundedPage, LayoutError> {
    let mut page = BoundedPage::new();
    layout_xhtml_page_into(encoded, requested_page, preferences, &mut page)?;
    Ok(page)
}

#[inline(always)]
pub fn layout_xhtml_page_into(
    encoded: &[u8],
    requested_page: usize,
    preferences: ReaderPreferences,
    page: &mut BoundedPage,
) -> Result<(), LayoutError> {
    layout_xhtml_page_with_images_into(encoded, requested_page, preferences, page, |_| None)
}

#[derive(Debug, Eq, PartialEq)]
pub enum StreamLayoutError<E> {
    Read(E),
    Write(E),
    Layout(LayoutError),
}

impl<E> From<XmlError> for StreamLayoutError<E> {
    fn from(error: XmlError) -> Self {
        Self::Layout(LayoutError::Xml(error))
    }
}

impl<E> From<LayoutError> for StreamLayoutError<E> {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChapterSummary {
    pub page_count: usize,
    pub title: FixedString<MAX_CHAPTER_TITLE_BYTES>,
}

#[derive(Clone, Copy)]
enum PageSelection {
    One(usize),
    All,
}

impl PageSelection {
    fn includes(self, index: usize) -> bool {
        matches!(self, Self::All) || matches!(self, Self::One(requested) if requested == index)
    }
}

pub fn layout_xhtml_page_with_images_into(
    encoded: &[u8],
    requested_page: usize,
    preferences: ReaderPreferences,
    page: &mut BoundedPage,
    mut resolve_image: impl FnMut(&str) -> Option<ImageResource>,
) -> Result<(), LayoutError> {
    page.reset(requested_page);
    let mut layout = XhtmlLayout::new(PageSink::new(
        PageSelection::One(requested_page),
        preferences,
        page,
        |_: &BoundedPage| Ok::<(), core::convert::Infallible>(()),
    ));
    let mut reader = XmlReader::new(encoded)?;
    let result = (|| {
        while let Some(event) = reader.next_event()? {
            layout.event(event, &mut |href| Ok(resolve_image(href)))?;
        }
        layout.sink.finish()
    })();
    match result {
        Ok(_) => Ok(()),
        Err(StreamLayoutError::Layout(error)) => Err(error),
        Err(StreamLayoutError::Read(never) | StreamLayoutError::Write(never)) => match never {},
    }
}

pub fn layout_xhtml_stream<R: crate::zip_stream::ReadAt>(
    source: &R,
    workspace: &mut crate::bounded_xml::stream::XmlWorkspace,
    preferences: ReaderPreferences,
    page: &mut BoundedPage,
    mut resolve_image: impl FnMut(&str) -> Result<Option<ImageResource>, R::Error>,
    complete: impl FnMut(&BoundedPage) -> Result<(), R::Error>,
) -> Result<ChapterSummary, StreamLayoutError<R::Error>> {
    use crate::bounded_xml::stream::{StreamXmlError, XmlStream};
    page.reset(0);
    let mut layout = XhtmlLayout::new(PageSink::new(
        PageSelection::All,
        preferences,
        page,
        complete,
    ));
    let mut reader = XmlStream::new(source, workspace);
    while let Some(event) = reader.next_event().map_err(|error| match error {
        StreamXmlError::Read(error) => StreamLayoutError::Read(error),
        StreamXmlError::Xml(error) => StreamLayoutError::from(error),
    })? {
        layout.event(event, &mut resolve_image)?;
    }
    layout.sink.finish()
}

struct XhtmlLayout<'a, F> {
    sink: PageSink<'a, F>,
    in_body: bool,
    hidden_depth: usize,
    quote_depth: usize,
    list_depth: usize,
}

impl<'a, E, F: FnMut(&BoundedPage) -> Result<(), E>> XhtmlLayout<'a, F> {
    fn new(sink: PageSink<'a, F>) -> Self {
        Self {
            sink,
            in_body: false,
            hidden_depth: 0,
            quote_depth: 0,
            list_depth: 0,
        }
    }

    fn event(
        &mut self,
        event: XmlEvent<'_>,
        resolve_image: &mut impl FnMut(&str) -> Result<Option<ImageResource>, E>,
    ) -> Result<(), StreamLayoutError<E>> {
        let Self {
            sink,
            in_body,
            hidden_depth,
            quote_depth,
            list_depth,
        } = self;
        match event {
            XmlEvent::Start(tag) => {
                let name = tag.local_name();
                if *hidden_depth > 0 {
                    *hidden_depth += 1;
                    return Ok(());
                }
                if name == "body" {
                    *in_body = true;
                    return Ok(());
                }
                if !*in_body {
                    return Ok(());
                }
                if matches!(name, "script" | "style") {
                    *hidden_depth = 1;
                    return Ok(());
                }
                match name {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        sink.begin_block(ReaderStyle::Heading)?
                    }
                    "blockquote" => {
                        *quote_depth += 1;
                        sink.begin_block(ReaderStyle::Quote)?;
                    }
                    "li" => {
                        *list_depth += 1;
                        sink.begin_block(ReaderStyle::ListItem)?;
                        sink.write_plain("• ")?;
                    }
                    "p" => sink.begin_block(contextual_style(*quote_depth, *list_depth))?,
                    "pre" => sink.begin_block(ReaderStyle::Preformatted)?,
                    "figcaption" | "caption" => sink.begin_block(ReaderStyle::Caption)?,
                    "tr" => sink.begin_block(ReaderStyle::Body)?,
                    "td" | "th" if !sink.line_is_empty() => sink.write_plain(" | ")?,
                    "br" => {
                        sink.line_break(true)?;
                    }
                    "img" | "image" => {
                        let href = tag
                            .attribute("src")?
                            .or(tag.attribute("href")?)
                            .or(tag.attribute("xlink:href")?);
                        let alt = tag.attribute("alt")?;
                        let image = match href {
                            Some(href) => resolve_image(href).map_err(StreamLayoutError::Read)?,
                            None => None,
                        };
                        if let Some(image) = image {
                            sink.image(image, alt)?;
                        } else {
                            append_image(alt, sink)?;
                        }
                    }
                    "hr" => {
                        sink.finish_block()?;
                        sink.begin_block(ReaderStyle::Body)?;
                        sink.write_plain("────────────────────────")?;
                        sink.finish_block()?;
                    }
                    _ => {}
                }
            }
            XmlEvent::Text(text) if *in_body && *hidden_depth == 0 => sink.write_text(text)?,
            XmlEvent::End(name) => {
                if *hidden_depth > 0 {
                    *hidden_depth -= 1;
                    return Ok(());
                }
                if name == "body" {
                    sink.finish_block()?;
                    *in_body = false;
                    return Ok(());
                }
                if !*in_body {
                    return Ok(());
                }
                match name {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "pre" | "figcaption"
                    | "caption" | "tr" => sink.finish_block()?,
                    "li" => {
                        sink.finish_block()?;
                        *list_depth = list_depth.saturating_sub(1);
                    }
                    "blockquote" => {
                        sink.finish_block()?;
                        *quote_depth = quote_depth.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            XmlEvent::Text(_) => {}
        }
        Ok(())
    }
}

const fn contextual_style(quote_depth: usize, list_depth: usize) -> ReaderStyle {
    if list_depth > 0 {
        ReaderStyle::ListItem
    } else if quote_depth > 0 {
        ReaderStyle::Quote
    } else {
        ReaderStyle::Body
    }
}

fn append_image<E, F: FnMut(&BoundedPage) -> Result<(), E>>(
    alt: Option<&str>,
    sink: &mut PageSink<'_, F>,
) -> Result<(), StreamLayoutError<E>> {
    if !sink.line_is_empty() {
        sink.write_plain(" ")?;
    }
    sink.write_plain("[Image")?;
    if let Some(alt) = alt.filter(|value| !value.trim().is_empty()) {
        sink.write_plain(": ")?;
        sink.write_encoded(alt)?;
    }
    sink.write_plain("]")
}

struct PageSink<'a, F> {
    selection: PageSelection,
    complete: F,
    page_index: usize,
    used_height: usize,
    page_line_count: usize,
    theme: ReaderTheme,
    page: &'a mut BoundedPage,
    current: FixedString<MAX_READER_LINE_BYTES>,
    current_width: usize,
    word: FixedString<MAX_READER_LINE_BYTES>,
    word_width: usize,
    previous_cr: bool,
    style: ReaderStyle,
    pending_space: bool,
    emitted_any: bool,
}

impl<'a, E, F: FnMut(&BoundedPage) -> Result<(), E>> PageSink<'a, F> {
    const fn new(
        selection: PageSelection,
        preferences: ReaderPreferences,
        page: &'a mut BoundedPage,
        complete: F,
    ) -> Self {
        Self {
            selection,
            complete,
            page_index: 0,
            used_height: 0,
            page_line_count: 0,
            theme: ReaderTheme::from_preferences(preferences),
            page,
            current: FixedString::new(),
            current_width: 0,
            word: FixedString::new(),
            word_width: 0,
            previous_cr: false,
            style: ReaderStyle::Body,
            pending_space: false,
            emitted_any: false,
        }
    }

    fn image(
        &mut self,
        resource: ImageResource,
        alt: Option<&str>,
    ) -> Result<(), StreamLayoutError<E>> {
        self.line_break(false)?;
        let content_width = resource
            .size
            .width()
            .min(crate::reader::BODY_WIDTH_PIXELS / 8 * 8);
        let width = content_width.div_ceil(8) * 8;
        let height = (resource.size.height() as u64).saturating_mul(content_width as u64)
            / resource.size.width() as u64;
        let height = height.max(1).min(PAGE_HEIGHT as u64) as usize;
        let spec =
            ImageSpec::new(width, height, ScaleMode::Contain).ok_or(LayoutError::TooManyLines)?;
        self.reserve(height)?;
        if self.selection.includes(self.page_index) {
            let alt = match alt {
                Some(value) => FixedString::from_decoded(value).unwrap_or_default(),
                None => FixedString::new(),
            };
            self.page.lines[self.page.line_count as usize] =
                Some(PageElement::Image(BoundedImage {
                    resource,
                    spec,
                    top: self.used_height as u16,
                    alt,
                }));
            self.page.line_count += 1;
        }
        self.used_height += height;
        self.page_line_count += 1;
        self.emitted_any = true;
        Ok(())
    }

    fn begin_block(&mut self, style: ReaderStyle) -> Result<(), StreamLayoutError<E>> {
        self.line_break(false)?;
        self.style = style;
        self.pending_space = false;
        self.previous_cr = false;
        Ok(())
    }

    fn finish_block(&mut self) -> Result<(), StreamLayoutError<E>> {
        let emitted = self.line_break(false)?;
        if emitted {
            self.emit(FixedString::new(), ReaderStyle::Body)?;
        }
        self.style = ReaderStyle::Body;
        self.pending_space = false;
        self.previous_cr = false;
        Ok(())
    }

    fn write_encoded(&mut self, value: &str) -> Result<(), StreamLayoutError<E>> {
        self.write_text(XmlText::Encoded(value))
    }

    fn write_text(&mut self, text: XmlText<'_>) -> Result<(), StreamLayoutError<E>> {
        for character in text {
            self.write_character(character?)?;
        }
        Ok(())
    }

    fn write_plain(&mut self, value: &str) -> Result<(), StreamLayoutError<E>> {
        for character in value.chars() {
            self.write_character(character)?;
        }
        Ok(())
    }

    fn write_character(&mut self, character: char) -> Result<(), StreamLayoutError<E>> {
        if self.style == ReaderStyle::Preformatted {
            return self.write_preformatted(character);
        }
        if matches!(character, ' ' | '\t' | '\r' | '\n') {
            self.flush_word()?;
            self.pending_space = !self.current.is_empty();
            return Ok(());
        }
        let width = self.theme.character_width(self.style, character);
        if !self.word.is_empty()
            && (self.word_width + width > self.theme.line_width(self.style)
                || self.word.as_str().len() + character.len_utf8() > MAX_READER_LINE_BYTES)
        {
            self.flush_word()?;
        }
        self.word.push(character)?;
        self.word_width += width;
        Ok(())
    }

    fn flush_word(&mut self) -> Result<(), StreamLayoutError<E>> {
        if self.word.is_empty() {
            return Ok(());
        }
        let word = core::mem::take(&mut self.word);
        let width = core::mem::replace(&mut self.word_width, 0);
        let space = usize::from(self.pending_space && !self.current.is_empty());
        if !self.current.is_empty()
            && (self.current_width + space * self.theme.character_width(self.style, ' ') + width
                > self.theme.line_width(self.style)
                || self.current.as_str().len() + space + word.as_str().len()
                    > MAX_READER_LINE_BYTES)
        {
            self.emit_current(false)?;
        }
        if self.pending_space && !self.current.is_empty() {
            self.current.push(' ')?;
            self.current_width += self.theme.character_width(self.style, ' ');
        }
        self.pending_space = false;
        self.current.push_str(word.as_str())?;
        self.current_width += width;
        Ok(())
    }

    fn write_preformatted(&mut self, character: char) -> Result<(), StreamLayoutError<E>> {
        if self.previous_cr && character == '\n' {
            self.previous_cr = false;
            return Ok(());
        }
        self.previous_cr = character == '\r';
        match character {
            '\r' | '\n' => {
                self.line_break(true)?;
            }
            '\t' => {
                let spaces = 4 - self.current.as_str().chars().count() % 4;
                for _ in 0..spaces {
                    self.push_preformatted(' ')?;
                }
            }
            character => self.push_preformatted(character)?,
        }
        Ok(())
    }

    fn push_preformatted(&mut self, character: char) -> Result<(), StreamLayoutError<E>> {
        let width = self.theme.character_width(self.style, character);
        if !self.current.is_empty()
            && (self.current_width + width > self.theme.line_width(self.style)
                || self.current.as_str().len() + character.len_utf8() > MAX_READER_LINE_BYTES)
        {
            self.emit_current(false)?;
        }
        self.current.push(character)?;
        self.current_width += width;
        Ok(())
    }

    fn line_is_empty(&self) -> bool {
        self.current.is_empty() && self.word.is_empty()
    }

    fn line_break(&mut self, force_empty: bool) -> Result<bool, StreamLayoutError<E>> {
        self.flush_word()?;
        self.emit_current(force_empty)
    }

    fn emit_current(&mut self, force_empty: bool) -> Result<bool, StreamLayoutError<E>> {
        self.pending_space = false;
        if self.current.is_empty() && !force_empty {
            return Ok(false);
        }
        let line = self.current;
        self.current.clear();
        self.current_width = 0;
        self.emit(line, self.style)?;
        Ok(true)
    }

    fn emit(
        &mut self,
        line: FixedString<MAX_READER_LINE_BYTES>,
        style: ReaderStyle,
    ) -> Result<(), StreamLayoutError<E>> {
        let height = self.theme.line_height(style);
        self.reserve(height)?;
        if self.selection.includes(self.page_index) {
            let index = usize::from(self.page.line_count);
            if index == MAX_PAGE_LINES {
                return Err(LayoutError::TooManyLines.into());
            }
            if style == ReaderStyle::Heading && self.page.chapter_title.is_empty() {
                self.page.chapter_title = copy_fixed(line.as_str())?;
            }
            self.page.lines[index] = Some(PageElement::Text(BoundedReaderLine {
                text: line,
                style,
                top: self.used_height as u16,
            }));
            self.page.line_count += 1;
        } else if style == ReaderStyle::Heading && self.page.chapter_title.is_empty() {
            self.page.chapter_title = copy_fixed(line.as_str())?;
        }
        self.used_height += height;
        self.page_line_count += 1;
        self.emitted_any = true;
        Ok(())
    }

    fn reserve(&mut self, height: usize) -> Result<(), StreamLayoutError<E>> {
        if self.used_height + height > PAGE_HEIGHT || self.page_line_count == MAX_PAGE_LINES {
            if matches!(self.selection, PageSelection::All) {
                (self.complete)(self.page).map_err(StreamLayoutError::Write)?;
                self.page.lines.fill(None);
                self.page.line_count = 0;
            }
            self.page_index = self
                .page_index
                .checked_add(1)
                .ok_or(LayoutError::TooManyLines)?;
            if matches!(self.selection, PageSelection::All) {
                self.page.page_index = self.page_index;
            }
            self.used_height = 0;
            self.page_line_count = 0;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<ChapterSummary, StreamLayoutError<E>> {
        self.line_break(false)?;
        let page_count = if self.emitted_any {
            self.page_index + 1
        } else {
            1
        };
        if matches!(self.selection, PageSelection::One(requested) if requested >= page_count) {
            return Err(LayoutError::PageOutOfBounds.into());
        }
        if self.page.line_count == 0 {
            let text = FixedString::try_from_str("This section contains no readable text.")?;
            self.page.lines[0] = Some(PageElement::Text(BoundedReaderLine {
                text,
                style: ReaderStyle::Body,
                top: 0,
            }));
            self.page.line_count = 1;
        }
        if self.page.chapter_title.is_empty() {
            self.page.chapter_title = FixedString::try_from_str("Section")?;
        }
        self.page.page_count = page_count;
        if matches!(self.selection, PageSelection::All) {
            (self.complete)(self.page).map_err(StreamLayoutError::Write)?;
        }
        Ok(ChapterSummary {
            page_count,
            title: self.page.chapter_title,
        })
    }
}

fn copy_fixed<const CAPACITY: usize>(value: &str) -> Result<FixedString<CAPACITY>, LayoutError> {
    let mut output = FixedString::new();
    for character in value.chars() {
        if output.push(character).is_err() {
            break;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod regression_tests;

#[cfg(test)]
mod image_tests;

#[cfg(test)]
mod stream_tests;

#[cfg(test)]
mod tests {
    extern crate std;

    use std::{string::String, vec::Vec};

    use super::{BoundedPage, LayoutError, layout_xhtml_page, layout_xhtml_page_into};
    use crate::{app::ReaderPreferences, reader::ReaderStyle};

    #[test]
    fn all_typography_choices_paginate_within_the_shared_line_budget() {
        use crate::app::{ReaderFont, ReaderFontSize, ReaderSpacing};
        let text = String::from("<html><body>")
            + &"<p>Words on a line.</p>".repeat(120)
            + "</body></html>";
        for font in [ReaderFont::NotoSerif, ReaderFont::Compact, ReaderFont::Mono] {
            for size in [
                ReaderFontSize::Small,
                ReaderFontSize::Medium,
                ReaderFontSize::Large,
            ] {
                for spacing in [
                    ReaderSpacing::Compact,
                    ReaderSpacing::Normal,
                    ReaderSpacing::Relaxed,
                ] {
                    let preferences = ReaderPreferences::new(font, size, spacing);
                    let first = layout_xhtml_page(text.as_bytes(), 0, preferences).unwrap();
                    assert!(first.page_count() > 1);
                    let mut paragraphs = 0;
                    for index in 0..first.page_count() {
                        let page = layout_xhtml_page(text.as_bytes(), index, preferences).unwrap();
                        assert!(page.lines().count() <= super::MAX_PAGE_LINES);
                        assert_eq!(page.page_count(), first.page_count());
                        paragraphs += page
                            .lines()
                            .filter(|line| line.text() == "Words on a line.")
                            .count();
                    }
                    assert_eq!(paragraphs, 120);
                }
            }
        }
    }

    #[test]
    fn preserves_unknown_text_images_lists_tables_and_quotes() {
        let xml = br#"<html><head><style>hidden</style></head><body>
<h1>A &amp; B</h1><p>Known <future>and future</future> words.</p>
<ul><li>one</li></ul><blockquote><p>quoted</p></blockquote>
<figure><img alt="diagram"/><figcaption>caption</figcaption></figure>
<table><tr><td>x</td><td>y</td></tr></table><script>hidden()</script>
</body></html>"#;
        let page = layout_xhtml_page(xml, 0, ReaderPreferences::default()).unwrap();
        let lines = page
            .lines()
            .map(|line| line.text())
            .collect::<Vec<_>>()
            .join(" ");

        assert_eq!(page.chapter_title(), "A & B");
        assert!(lines.contains("Known and future words."));
        assert!(lines.contains("• one"));
        assert!(lines.contains("quoted"));
        assert!(lines.contains("[Image: diagram]"));
        assert!(lines.contains("x | y"));
        assert!(!lines.contains("hidden"));
        assert!(page.lines().any(|line| line.style() == ReaderStyle::Quote));
    }

    #[test]
    fn paginates_without_retaining_a_chapter_dom() {
        let paragraph = "bounded words ".repeat(4_000);
        let xml = String::from("<html><body><p>") + &paragraph + "</p></body></html>";
        let mut page = BoundedPage::new();
        layout_xhtml_page_into(xml.as_bytes(), 0, ReaderPreferences::default(), &mut page).unwrap();
        let page_count = page.page_count();

        assert!(page_count > 1);

        layout_xhtml_page_into(
            xml.as_bytes(),
            page_count - 1,
            ReaderPreferences::default(),
            &mut page,
        )
        .unwrap();
        assert_eq!(page.page_count(), page_count);
        assert_eq!(page.page_index(), page_count - 1);
        assert!(!page.lines().collect::<Vec<_>>().is_empty());
        assert_eq!(
            layout_xhtml_page_into(
                xml.as_bytes(),
                page_count,
                ReaderPreferences::default(),
                &mut page,
            ),
            Err(LayoutError::PageOutOfBounds)
        );
    }
}
