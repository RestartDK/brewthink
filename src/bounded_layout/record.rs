use super::*;

pub const MAX_ENCODED_PAGE_BYTES: usize = 20 * 1024;

struct Writer<'a> {
    bytes: &'a mut [u8],
    position: usize,
}

impl Writer<'_> {
    fn bytes(&mut self, bytes: &[u8]) -> Option<()> {
        let end = self.position.checked_add(bytes.len())?;
        self.bytes
            .get_mut(self.position..end)?
            .copy_from_slice(bytes);
        self.position = end;
        Some(())
    }
    fn byte(&mut self, value: u8) -> Option<()> {
        self.bytes(&[value])
    }
    fn u16(&mut self, value: u16) -> Option<()> {
        self.bytes(&value.to_le_bytes())
    }
    fn u32(&mut self, value: u32) -> Option<()> {
        self.bytes(&value.to_le_bytes())
    }
    fn text(&mut self, value: &str) -> Option<()> {
        self.u16(u16::try_from(value.len()).ok()?)?;
        self.bytes(value.as_bytes())
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(count)?;
        let bytes = self.bytes.get(self.position..end)?;
        self.position = end;
        Some(bytes)
    }
    fn byte(&mut self) -> Option<u8> {
        Some(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.bytes(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
    fn text<const N: usize>(&mut self) -> Option<FixedString<N>> {
        let length = usize::from(self.u16()?);
        let text = core::str::from_utf8(self.bytes(length)?).ok()?;
        if !text.chars().all(crate::bounded_xml::is_xml_character) {
            return None;
        }
        FixedString::try_from_str(text).ok()
    }
}

impl BoundedPage {
    pub fn encode(&self, output: &mut [u8]) -> Option<usize> {
        let mut out = Writer {
            bytes: output,
            position: 0,
        };
        out.byte(self.line_count)?;
        for element in self.lines[..usize::from(self.line_count)].iter() {
            match element.as_ref()? {
                PageElement::Text(line) => {
                    out.byte(0)?;
                    out.u16(line.top)?;
                    out.byte(match line.style {
                        ReaderStyle::Body => 0,
                        ReaderStyle::Heading => 1,
                        ReaderStyle::Quote => 2,
                        ReaderStyle::ListItem => 3,
                        ReaderStyle::Preformatted => 4,
                        ReaderStyle::Caption => 5,
                    })?;
                    out.text(line.text())?;
                }
                PageElement::Image(image) => {
                    out.byte(1)?;
                    out.u16(image.top)?;
                    out.u16(u16::try_from(image.resource.size.width()).ok()?)?;
                    out.u16(u16::try_from(image.resource.size.height()).ok()?)?;
                    out.u16(image.spec.size().width() as u16)?;
                    out.u16(image.spec.size().height() as u16)?;
                    out.u32(image.resource.crc32)?;
                    out.u32(image.resource.bytes)?;
                    out.text(image.path())?;
                    out.text(image.alt())?;
                }
            }
        }
        Some(out.position)
    }

    pub fn decode(
        &mut self,
        bytes: &[u8],
        page_index: usize,
        summary: ChapterSummary,
        preferences: ReaderPreferences,
    ) -> Option<()> {
        if page_index >= summary.page_count {
            return None;
        }
        let mut input = Reader { bytes, position: 0 };
        let count = usize::from(input.byte()?);
        if count == 0 || count > MAX_PAGE_LINES {
            return None;
        }
        self.reset(page_index);
        let theme = ReaderTheme::from_preferences(preferences);
        let mut bottom = 0usize;
        for index in 0..count {
            let kind = input.byte()?;
            let top = input.u16()?;
            if usize::from(top) < bottom {
                return None;
            }
            let (element, height) = match kind {
                0 => {
                    let style = match input.byte()? {
                        0 => ReaderStyle::Body,
                        1 => ReaderStyle::Heading,
                        2 => ReaderStyle::Quote,
                        3 => ReaderStyle::ListItem,
                        4 => ReaderStyle::Preformatted,
                        5 => ReaderStyle::Caption,
                        _ => return None,
                    };
                    let text = input.text()?;
                    (
                        PageElement::Text(BoundedReaderLine { text, style, top }),
                        theme.line_height(style),
                    )
                }
                1 => {
                    let width = usize::from(input.u16()?);
                    let height = usize::from(input.u16()?);
                    let size = crate::image_decoder::checked_size(width, height).ok()?;
                    let width = usize::from(input.u16()?);
                    let height = usize::from(input.u16()?);
                    if width > crate::reader::BODY_WIDTH_PIXELS / 8 * 8 {
                        return None;
                    }
                    let spec = ImageSpec::new(width, height, ScaleMode::Contain)?;
                    let crc32 = input.u32()?;
                    let bytes = input.u32()?;
                    let path = input.text()?;
                    let alt = input.text()?;
                    if path.is_empty() {
                        return None;
                    }
                    let resource = ImageResource {
                        path,
                        size,
                        crc32,
                        bytes,
                    };
                    (
                        PageElement::Image(BoundedImage {
                            resource,
                            spec,
                            top,
                            alt,
                        }),
                        height,
                    )
                }
                _ => return None,
            };
            bottom = usize::from(top).checked_add(height)?;
            if bottom > PAGE_HEIGHT {
                return None;
            }
            self.lines[index] = Some(element);
        }
        if input.position != bytes.len() {
            return None;
        }
        self.line_count = count as u8;
        self.page_count = summary.page_count;
        self.chapter_title = summary.title;
        Some(())
    }
}
