#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XmlError {
    InvalidUtf8,
    Malformed,
    InvalidEntity,
    OutputFull,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XmlTag<'a> {
    source: &'a str,
    name_end: usize,
    empty: bool,
}

impl<'a> XmlTag<'a> {
    pub fn local_name(self) -> &'a str {
        local_name(&self.source[..self.name_end])
    }

    pub const fn is_empty(self) -> bool {
        self.empty
    }

    pub fn attribute(self, requested: &str) -> Result<Option<&'a str>, XmlError> {
        let bytes = self.source.as_bytes();
        let mut cursor = self.name_end;
        while cursor < bytes.len() {
            skip_whitespace(bytes, &mut cursor);
            if cursor == bytes.len() || bytes[cursor] == b'/' {
                return Ok(None);
            }
            let name_start = cursor;
            while cursor < bytes.len()
                && !bytes[cursor].is_ascii_whitespace()
                && !matches!(bytes[cursor], b'=' | b'/')
            {
                cursor += 1;
            }
            if cursor == name_start {
                return Err(XmlError::Malformed);
            }
            let name = core::str::from_utf8(&bytes[name_start..cursor])
                .map_err(|_| XmlError::InvalidUtf8)?;
            skip_whitespace(bytes, &mut cursor);
            if bytes.get(cursor) != Some(&b'=') {
                return Err(XmlError::Malformed);
            }
            cursor += 1;
            skip_whitespace(bytes, &mut cursor);
            let quote = *bytes.get(cursor).ok_or(XmlError::Malformed)?;
            if !matches!(quote, b'\'' | b'"') {
                return Err(XmlError::Malformed);
            }
            cursor += 1;
            let value_start = cursor;
            while bytes.get(cursor).is_some_and(|byte| *byte != quote) {
                cursor += 1;
            }
            if bytes.get(cursor) != Some(&quote) {
                return Err(XmlError::Malformed);
            }
            let value = core::str::from_utf8(&bytes[value_start..cursor])
                .map_err(|_| XmlError::InvalidUtf8)?;
            cursor += 1;
            if local_name(name) == requested {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XmlEvent<'a> {
    Start(XmlTag<'a>),
    End(&'a str),
    Text(&'a str),
}

pub struct XmlReader<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> XmlReader<'a> {
    pub fn new(encoded: &'a [u8]) -> Result<Self, XmlError> {
        let source = core::str::from_utf8(encoded).map_err(|_| XmlError::InvalidUtf8)?;
        Ok(Self { source, cursor: 0 })
    }

    pub fn next_event(&mut self) -> Result<Option<XmlEvent<'a>>, XmlError> {
        loop {
            if self.cursor == self.source.len() {
                return Ok(None);
            }
            let remaining = &self.source[self.cursor..];
            let Some(relative_tag) = remaining.find('<') else {
                self.cursor = self.source.len();
                return Ok((!remaining.is_empty()).then_some(XmlEvent::Text(remaining)));
            };
            if relative_tag > 0 {
                let start = self.cursor;
                self.cursor += relative_tag;
                return Ok(Some(XmlEvent::Text(&self.source[start..self.cursor])));
            }
            if remaining.starts_with("<!--") {
                self.skip_through("-->")?;
                continue;
            }
            if remaining.starts_with("<![CDATA[") {
                let start = self.cursor + 9;
                let tail = &self.source[start..];
                let length = tail.find("]]>").ok_or(XmlError::Malformed)?;
                self.cursor = start + length + 3;
                return Ok(Some(XmlEvent::Text(&self.source[start..start + length])));
            }
            if remaining.starts_with("<?") {
                self.skip_through("?>")?;
                continue;
            }
            if remaining.starts_with("<!") {
                self.skip_declaration()?;
                continue;
            }
            let end = find_tag_end(remaining).ok_or(XmlError::Malformed)?;
            let source = &remaining[1..end];
            self.cursor += end + 1;
            let source = source.trim();
            if let Some(end_name) = source.strip_prefix('/') {
                let name = end_name.trim();
                if name.is_empty() || name.bytes().any(|byte| byte.is_ascii_whitespace()) {
                    return Err(XmlError::Malformed);
                }
                return Ok(Some(XmlEvent::End(local_name(name))));
            }
            let empty = source.ends_with('/');
            let source = if empty {
                source[..source.len() - 1].trim_end()
            } else {
                source
            };
            let name_end = source
                .bytes()
                .position(|byte| byte.is_ascii_whitespace())
                .unwrap_or(source.len());
            if name_end == 0 {
                return Err(XmlError::Malformed);
            }
            return Ok(Some(XmlEvent::Start(XmlTag {
                source,
                name_end,
                empty,
            })));
        }
    }

    fn skip_through(&mut self, delimiter: &str) -> Result<(), XmlError> {
        let remaining = &self.source[self.cursor..];
        let end = remaining.find(delimiter).ok_or(XmlError::Malformed)?;
        self.cursor += end + delimiter.len();
        Ok(())
    }

    fn skip_declaration(&mut self) -> Result<(), XmlError> {
        let remaining = &self.source[self.cursor..];
        let mut quote = None;
        let mut bracket_depth = 0usize;
        for (index, character) in remaining.char_indices().skip(2) {
            match (quote, character) {
                (Some(active), value) if active == value => quote = None,
                (Some(_), _) => {}
                (None, '\'' | '"') => quote = Some(character),
                (None, '[') => bracket_depth += 1,
                (None, ']') => bracket_depth = bracket_depth.saturating_sub(1),
                (None, '>') if bracket_depth == 0 => {
                    self.cursor += index + 1;
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(XmlError::Malformed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedString<const CAPACITY: usize> {
    bytes: [u8; CAPACITY],
    length: u16,
}

impl<const CAPACITY: usize> FixedString<CAPACITY> {
    pub const fn new() -> Self {
        Self {
            bytes: [0; CAPACITY],
            length: 0,
        }
    }

    pub fn try_from_str(value: &str) -> Result<Self, XmlError> {
        let mut output = Self::new();
        output.push_str(value)?;
        Ok(output)
    }

    pub fn from_decoded(value: &str) -> Result<Self, XmlError> {
        let mut output = Self::new();
        decode_entities(value, |character| output.push(character))?;
        Ok(output)
    }

    pub fn push(&mut self, character: char) -> Result<(), XmlError> {
        let mut encoded = [0; 4];
        let bytes = character.encode_utf8(&mut encoded).as_bytes();
        let start = usize::from(self.length);
        let end = start.checked_add(bytes.len()).ok_or(XmlError::OutputFull)?;
        if end > CAPACITY {
            return Err(XmlError::OutputFull);
        }
        self.bytes[start..end].copy_from_slice(bytes);
        self.length = end as u16;
        Ok(())
    }

    pub fn push_str(&mut self, value: &str) -> Result<(), XmlError> {
        for character in value.chars() {
            self.push(character)?;
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        self.length = 0;
    }

    pub fn trim_whitespace(&mut self) {
        let value = self.as_str();
        let trimmed = value.trim();
        let start = trimmed.as_ptr() as usize - value.as_ptr() as usize;
        let length = trimmed.len();
        self.bytes.copy_within(start..start + length, 0);
        self.length = length as u16;
    }

    pub fn normalize_whitespace(&mut self) {
        let original_length = usize::from(self.length);
        let mut read = 0;
        let mut write = 0;
        let mut pending_space = false;
        while read < original_length {
            let value = core::str::from_utf8(&self.bytes[read..original_length])
                .expect("fixed strings contain valid UTF-8");
            let character = value
                .chars()
                .next()
                .expect("the remaining value is non-empty");
            read += character.len_utf8();
            if character.is_whitespace() {
                pending_space = write > 0;
                continue;
            }
            if pending_space {
                self.bytes[write] = b' ';
                write += 1;
                pending_space = false;
            }
            let mut encoded = [0; 4];
            let bytes = character.encode_utf8(&mut encoded).as_bytes();
            self.bytes.copy_within(read - bytes.len()..read, write);
            write += bytes.len();
        }
        self.length = write as u16;
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..usize::from(self.length)])
            .expect("fixed strings only receive valid UTF-8 characters")
    }

    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }
}

impl<const CAPACITY: usize> Default for FixedString<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAPACITY: usize> core::fmt::Write for FixedString<CAPACITY> {
    fn write_str(&mut self, value: &str) -> core::fmt::Result {
        self.push_str(value).map_err(|_| core::fmt::Error)
    }
}

pub fn decode_entities(
    value: &str,
    mut output: impl FnMut(char) -> Result<(), XmlError>,
) -> Result<(), XmlError> {
    let mut remaining = value;
    while let Some(entity_start) = remaining.find('&') {
        for character in remaining[..entity_start].chars() {
            output(character)?;
        }
        remaining = &remaining[entity_start + 1..];
        let entity_end = remaining.find(';').ok_or(XmlError::InvalidEntity)?;
        let entity = &remaining[..entity_end];
        let character = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            numeric if numeric.starts_with("#x") || numeric.starts_with("#X") => char::from_u32(
                u32::from_str_radix(&numeric[2..], 16).map_err(|_| XmlError::InvalidEntity)?,
            )
            .ok_or(XmlError::InvalidEntity)?,
            numeric if numeric.starts_with('#') => char::from_u32(
                numeric[1..]
                    .parse::<u32>()
                    .map_err(|_| XmlError::InvalidEntity)?,
            )
            .ok_or(XmlError::InvalidEntity)?,
            _ => return Err(XmlError::InvalidEntity),
        };
        output(character)?;
        remaining = &remaining[entity_end + 1..];
    }
    for character in remaining.chars() {
        output(character)?;
    }
    Ok(())
}

fn local_name(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

fn skip_whitespace(bytes: &[u8], cursor: &mut usize) {
    while bytes
        .get(*cursor)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        *cursor += 1;
    }
}

fn find_tag_end(value: &str) -> Option<usize> {
    let mut quote = None;
    for (index, character) in value.char_indices().skip(1) {
        match (quote, character) {
            (Some(active), value) if active == value => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(character),
            (None, '>') => return Some(index),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{FixedString, XmlEvent, XmlReader};

    #[test]
    fn reads_namespaced_tags_attributes_entities_and_cdata() {
        let xml = br#"<?xml version="1.0"?><root><!-- no --><dc:title role="main">A &amp; B</dc:title><item href="a>b"/><![CDATA[tail]]></root>"#;
        let mut reader = XmlReader::new(xml).unwrap();
        let Some(XmlEvent::Start(root)) = reader.next_event().unwrap() else {
            panic!("root expected");
        };
        assert_eq!(root.local_name(), "root");
        let Some(XmlEvent::Start(title)) = reader.next_event().unwrap() else {
            panic!("title expected");
        };
        assert_eq!(title.local_name(), "title");
        assert_eq!(title.attribute("role").unwrap(), Some("main"));
        let Some(XmlEvent::Text(text)) = reader.next_event().unwrap() else {
            panic!("title text expected");
        };
        assert_eq!(
            FixedString::<16>::from_decoded(text).unwrap().as_str(),
            "A & B"
        );
        assert!(matches!(
            reader.next_event().unwrap(),
            Some(XmlEvent::End("title"))
        ));
        let Some(XmlEvent::Start(item)) = reader.next_event().unwrap() else {
            panic!("item expected");
        };
        assert!(item.is_empty());
        assert_eq!(item.attribute("href").unwrap(), Some("a>b"));
        assert_eq!(reader.next_event().unwrap(), Some(XmlEvent::Text("tail")));
    }
}
