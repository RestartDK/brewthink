use crate::{
    app::ReaderPreferences,
    bounded_xml::{FixedString, XmlError, XmlEvent, XmlReader, decode_entities},
    reader::{ReaderStyle, ReaderTheme},
};

pub const MAX_PAGE_LINES: usize = 50;
pub const MAX_READER_LINE_BYTES: usize = 320;
pub const MAX_CHAPTER_TITLE_BYTES: usize = 96;
const PAGE_HEIGHT: usize = crate::reader::BODY_BOTTOM - crate::reader::BODY_TOP;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedReaderLine {
    text: FixedString<MAX_READER_LINE_BYTES>,
    style: ReaderStyle,
}

impl BoundedReaderLine {
    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    pub const fn style(&self) -> ReaderStyle {
        self.style
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundedPage {
    lines: [Option<BoundedReaderLine>; MAX_PAGE_LINES],
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
        self.lines[..usize::from(self.line_count)].iter().flatten()
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
    page.reset(requested_page);
    let mut sink = PageSink::new(requested_page, preferences, page);
    let mut reader = XmlReader::new(encoded)?;
    let mut in_body = false;
    let mut hidden_depth = 0usize;
    let mut quote_depth = 0usize;
    let mut list_depth = 0usize;

    while let Some(event) = reader.next_event()? {
        match event {
            XmlEvent::Start(tag) => {
                let name = tag.local_name();
                if hidden_depth > 0 {
                    if !tag.is_empty() {
                        hidden_depth += 1;
                    }
                    continue;
                }
                if name == "body" {
                    in_body = true;
                    continue;
                }
                if !in_body {
                    continue;
                }
                if matches!(name, "script" | "style") {
                    if !tag.is_empty() {
                        hidden_depth = 1;
                    }
                    continue;
                }
                match name {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        sink.begin_block(ReaderStyle::Heading)?
                    }
                    "blockquote" => {
                        quote_depth += 1;
                        sink.begin_block(ReaderStyle::Quote)?;
                    }
                    "li" => {
                        list_depth += 1;
                        sink.begin_block(ReaderStyle::ListItem)?;
                        sink.write_plain("• ")?;
                    }
                    "p" => sink.begin_block(contextual_style(quote_depth, list_depth))?,
                    "pre" => sink.begin_block(ReaderStyle::Preformatted)?,
                    "figcaption" | "caption" => sink.begin_block(ReaderStyle::Caption)?,
                    "tr" => sink.begin_block(ReaderStyle::Body)?,
                    "td" | "th" if !sink.line_is_empty() => sink.write_plain(" | ")?,
                    "br" => {
                        sink.line_break(true)?;
                    }
                    "img" => append_image(tag.attribute("alt")?, &mut sink)?,
                    "hr" => {
                        sink.finish_block()?;
                        sink.begin_block(ReaderStyle::Body)?;
                        sink.write_plain("────────────────────────")?;
                        sink.finish_block()?;
                    }
                    _ => {}
                }
            }
            XmlEvent::Text(text) if in_body && hidden_depth == 0 => sink.write_encoded(text)?,
            XmlEvent::End(name) => {
                if hidden_depth > 0 {
                    hidden_depth -= 1;
                    continue;
                }
                if name == "body" {
                    sink.finish_block()?;
                    in_body = false;
                    continue;
                }
                if !in_body {
                    continue;
                }
                match name {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "pre" | "figcaption"
                    | "caption" | "tr" => sink.finish_block()?,
                    "li" => {
                        sink.finish_block()?;
                        list_depth = list_depth.saturating_sub(1);
                    }
                    "blockquote" => {
                        sink.finish_block()?;
                        quote_depth = quote_depth.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            XmlEvent::Text(_) => {}
        }
    }
    sink.finish()
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

fn append_image(alt: Option<&str>, sink: &mut PageSink<'_>) -> Result<(), LayoutError> {
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

struct PageSink<'a> {
    requested_page: usize,
    page_index: usize,
    used_height: usize,
    theme: ReaderTheme,
    page: &'a mut BoundedPage,
    current: FixedString<MAX_READER_LINE_BYTES>,
    style: ReaderStyle,
    pending_space: bool,
    emitted_any: bool,
}

impl<'a> PageSink<'a> {
    const fn new(
        requested_page: usize,
        preferences: ReaderPreferences,
        page: &'a mut BoundedPage,
    ) -> Self {
        Self {
            requested_page,
            page_index: 0,
            used_height: 0,
            theme: ReaderTheme::from_preferences(preferences),
            page,
            current: FixedString::new(),
            style: ReaderStyle::Body,
            pending_space: false,
            emitted_any: false,
        }
    }

    fn begin_block(&mut self, style: ReaderStyle) -> Result<(), LayoutError> {
        self.line_break(false)?;
        self.style = style;
        self.pending_space = false;
        Ok(())
    }

    fn finish_block(&mut self) -> Result<(), LayoutError> {
        let emitted = self.line_break(false)?;
        if emitted {
            self.emit(FixedString::new(), ReaderStyle::Body)?;
        }
        self.style = ReaderStyle::Body;
        self.pending_space = false;
        Ok(())
    }

    fn write_encoded(&mut self, value: &str) -> Result<(), LayoutError> {
        decode_entities(value, |character| self.write_character(character))?;
        Ok(())
    }

    fn write_plain(&mut self, value: &str) -> Result<(), LayoutError> {
        for character in value.chars() {
            self.write_character(character)?;
        }
        Ok(())
    }

    fn write_character(&mut self, character: char) -> Result<(), XmlError> {
        if character.is_whitespace() {
            self.pending_space = !self.current.is_empty();
            return Ok(());
        }
        let width = self.theme.characters_per_line(self.style);
        if self.pending_space {
            if self.current.as_str().chars().count() + 1 > width {
                self.line_break(false).map_err(layout_to_xml)?;
            } else if !self.current.is_empty() {
                self.current.push(' ')?;
            }
            self.pending_space = false;
        }
        if self.current.as_str().chars().count() == width {
            self.line_break(false).map_err(layout_to_xml)?;
        }
        self.current.push(character)
    }

    fn line_is_empty(&self) -> bool {
        self.current.is_empty()
    }

    fn line_break(&mut self, force_empty: bool) -> Result<bool, LayoutError> {
        self.pending_space = false;
        if self.current.is_empty() && !force_empty {
            return Ok(false);
        }
        let line = self.current;
        self.current.clear();
        self.emit(line, self.style)?;
        Ok(true)
    }

    fn emit(
        &mut self,
        line: FixedString<MAX_READER_LINE_BYTES>,
        style: ReaderStyle,
    ) -> Result<(), LayoutError> {
        let height = self.theme.line_height(style);
        if self.used_height + height > PAGE_HEIGHT {
            self.page_index += 1;
            self.used_height = 0;
        }
        if self.page_index == self.requested_page {
            let index = usize::from(self.page.line_count);
            if index == MAX_PAGE_LINES {
                return Err(LayoutError::TooManyLines);
            }
            if style == ReaderStyle::Heading && self.page.chapter_title.is_empty() {
                self.page.chapter_title = copy_fixed(line.as_str())?;
            }
            self.page.lines[index] = Some(BoundedReaderLine { text: line, style });
            self.page.line_count += 1;
        } else if style == ReaderStyle::Heading && self.page.chapter_title.is_empty() {
            self.page.chapter_title = copy_fixed(line.as_str())?;
        }
        self.used_height += height;
        self.emitted_any = true;
        Ok(())
    }

    fn finish(mut self) -> Result<(), LayoutError> {
        self.line_break(false)?;
        let page_count = if self.emitted_any {
            self.page_index + 1
        } else {
            1
        };
        if self.requested_page >= page_count {
            return Err(LayoutError::PageOutOfBounds);
        }
        if self.page.line_count == 0 {
            let text = FixedString::try_from_str("This section contains no readable text.")?;
            self.page.lines[0] = Some(BoundedReaderLine {
                text,
                style: ReaderStyle::Body,
            });
            self.page.line_count = 1;
        }
        if self.page.chapter_title.is_empty() {
            self.page.chapter_title = FixedString::try_from_str("Section")?;
        }
        self.page.page_count = page_count;
        Ok(())
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

fn layout_to_xml(error: LayoutError) -> XmlError {
    match error {
        LayoutError::Xml(error) => error,
        LayoutError::PageOutOfBounds | LayoutError::TooManyLines => XmlError::OutputFull,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::{string::String, vec::Vec};

    use super::{BoundedPage, LayoutError, layout_xhtml_page, layout_xhtml_page_into};
    use crate::{app::ReaderPreferences, reader::ReaderStyle};

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
