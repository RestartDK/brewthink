use super::{
    DocumentPhase, FixedString, MAX_XML_DEPTH, XmlError, XmlEvent, XmlTag, XmlText, decode_entity,
    is_xml_character, local_name,
};
use crate::zip_stream::ReadAt;

pub const INPUT_BYTES: usize = 1024;
pub const TOKEN_BYTES: usize = 4096;
const NAME_BYTES: usize = 128;

#[derive(Debug, Eq, PartialEq)]
pub enum StreamXmlError<E> {
    Read(E),
    Xml(XmlError),
}

impl<E> From<XmlError> for StreamXmlError<E> {
    fn from(error: XmlError) -> Self {
        Self::Xml(error)
    }
}

pub struct XmlWorkspace {
    input: [u8; INPUT_BYTES],
    token: FixedString<TOKEN_BYTES>,
    open: [FixedString<NAME_BYTES>; MAX_XML_DEPTH],
}

impl XmlWorkspace {
    pub const fn new() -> Self {
        Self {
            input: [0; INPUT_BYTES],
            token: FixedString::new(),
            open: [FixedString::new(); MAX_XML_DEPTH],
        }
    }

    pub(crate) unsafe fn initialize_in_place(storage: *mut Self) {
        // SAFETY: the caller supplies aligned exclusive storage; all fields are written.
        unsafe {
            core::ptr::addr_of_mut!((*storage).input).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*storage).token).write(FixedString::new());
            let names = core::ptr::addr_of_mut!((*storage).open).cast::<FixedString<NAME_BYTES>>();
            for index in 0..MAX_XML_DEPTH {
                names.add(index).write(FixedString::new());
            }
        }
    }
}

impl Default for XmlWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
enum TextMode {
    Encoded,
    Cdata { brackets: u8 },
}

pub struct XmlStream<'a, R> {
    source: &'a R,
    workspace: &'a mut XmlWorkspace,
    offset: u32,
    input_start: usize,
    input_end: usize,
    peeked: Option<char>,
    started: bool,
    depth: usize,
    phase: DocumentPhase,
    pending_end: Option<FixedString<NAME_BYTES>>,
    text_mode: TextMode,
}

impl<'a, R: ReadAt> XmlStream<'a, R> {
    pub fn new(source: &'a R, workspace: &'a mut XmlWorkspace) -> Self {
        workspace.token.clear();
        Self {
            source,
            workspace,
            offset: 0,
            input_start: 0,
            input_end: 0,
            peeked: None,
            started: false,
            depth: 0,
            phase: DocumentPhase::Prolog,
            pending_end: None,
            text_mode: TextMode::Encoded,
        }
    }

    fn byte(&mut self) -> Result<Option<u8>, StreamXmlError<R::Error>> {
        if self.input_start == self.input_end {
            if self.offset == self.source.len() {
                return Ok(None);
            }
            let capacity = INPUT_BYTES.min((self.source.len() - self.offset) as usize);
            let count = self
                .source
                .read_at(self.offset, &mut self.workspace.input[..capacity])
                .map_err(StreamXmlError::Read)?;
            if count == 0 || count > capacity {
                return Err(XmlError::Malformed.into());
            }
            self.offset += count as u32;
            self.input_start = 0;
            self.input_end = count;
        }
        let byte = self.workspace.input[self.input_start];
        self.input_start += 1;
        Ok(Some(byte))
    }

    fn character(&mut self) -> Result<Option<char>, StreamXmlError<R::Error>> {
        if let Some(character) = self.peeked.take() {
            return Ok(Some(character));
        }
        let Some(first) = self.byte()? else {
            return Ok(None);
        };
        let length = match first {
            0..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return Err(XmlError::InvalidUtf8.into()),
        };
        let mut bytes = [first, 0, 0, 0];
        for byte in &mut bytes[1..length] {
            *byte = self.byte()?.ok_or(XmlError::InvalidUtf8)?;
        }
        let text = core::str::from_utf8(&bytes[..length]).map_err(|_| XmlError::InvalidUtf8)?;
        let character = text.chars().next().ok_or(XmlError::InvalidUtf8)?;
        if !is_xml_character(character) {
            return Err(XmlError::Malformed.into());
        }
        Ok(Some(character))
    }

    fn peek(&mut self) -> Result<Option<char>, StreamXmlError<R::Error>> {
        if self.peeked.is_none() {
            self.peeked = self.character()?;
        }
        Ok(self.peeked)
    }

    fn expect(&mut self, expected: &str) -> Result<(), StreamXmlError<R::Error>> {
        for character in expected.chars() {
            if self.character()? != Some(character) {
                return Err(XmlError::Malformed.into());
            }
        }
        Ok(())
    }

    fn skip_through(&mut self, delimiter: &str) -> Result<(), StreamXmlError<R::Error>> {
        let mut tail = ['\0'; 3];
        loop {
            let character = self.character()?.ok_or(XmlError::Malformed)?;
            tail.rotate_left(1);
            tail[2] = character;
            if tail[3 - delimiter.len()..]
                .iter()
                .copied()
                .eq(delimiter.chars())
            {
                return Ok(());
            }
        }
    }

    fn declaration(&mut self) -> Result<(), StreamXmlError<R::Error>> {
        if self.phase != DocumentPhase::Prolog {
            return Err(XmlError::Malformed.into());
        }
        self.expect("DOCTYPE")?;
        let mut quote = None;
        let mut brackets = 0usize;
        loop {
            let character = self.character()?.ok_or(XmlError::Malformed)?;
            match (quote, character) {
                (Some(active), value) if active == value => quote = None,
                (Some(_), _) => {}
                (None, '\'' | '"') => quote = Some(character),
                (None, '[') => brackets = brackets.checked_add(1).ok_or(XmlError::Malformed)?,
                (None, ']') => brackets = brackets.saturating_sub(1),
                (None, '>') if brackets == 0 => return Ok(()),
                _ => {}
            }
        }
    }

    fn text(&mut self) -> Result<(), StreamXmlError<R::Error>> {
        while self.workspace.token.as_str().len() <= TOKEN_BYTES - 12 {
            match self.text_mode {
                TextMode::Encoded => {
                    let Some(character) = self.peek()? else {
                        break;
                    };
                    if character == '<' {
                        break;
                    }
                    self.character()?;
                    if self.depth == 0 && !matches!(character, ' ' | '\t' | '\r' | '\n') {
                        return Err(XmlError::Malformed.into());
                    }
                    let character = if character == '&' {
                        let mut entity = FixedString::<32>::new();
                        loop {
                            let character = self.character()?.ok_or(XmlError::InvalidEntity)?;
                            if character == ';' {
                                break;
                            }
                            entity
                                .push(character)
                                .map_err(|_| XmlError::InvalidEntity)?;
                        }
                        decode_entity(entity.as_str())?
                    } else {
                        character
                    };
                    self.workspace.token.push(character)?;
                }
                TextMode::Cdata { mut brackets } => {
                    let character = self.character()?.ok_or(XmlError::Malformed)?;
                    if character == ']' {
                        if brackets == 2 {
                            self.workspace.token.push(']')?;
                        } else {
                            brackets += 1;
                        }
                    } else if character == '>' && brackets == 2 {
                        self.text_mode = TextMode::Encoded;
                        break;
                    } else {
                        for _ in 0..brackets {
                            self.workspace.token.push(']')?;
                        }
                        brackets = 0;
                        self.workspace.token.push(character)?;
                    }
                    self.text_mode = TextMode::Cdata { brackets };
                }
            }
        }
        Ok(())
    }

    pub fn next_event(&mut self) -> Result<Option<XmlEvent<'_>>, StreamXmlError<R::Error>> {
        self.workspace.token.clear();
        if !self.started {
            self.started = true;
            if self.peek()? == Some('\u{feff}') {
                self.character()?;
            }
        }
        if let Some(name) = self.pending_end.take() {
            self.workspace.token.push_str(name.as_str())?;
            return Ok(Some(XmlEvent::End(local_name(
                self.workspace.token.as_str(),
            ))));
        }
        loop {
            if matches!(self.text_mode, TextMode::Cdata { .. })
                || self.peek()?.is_some_and(|c| c != '<')
            {
                self.text()?;
                return Ok(Some(XmlEvent::Text(XmlText::Literal(
                    self.workspace.token.as_str(),
                ))));
            }
            if self.peek()?.is_none() {
                return if self.phase == DocumentPhase::Epilog {
                    Ok(None)
                } else {
                    Err(XmlError::Malformed.into())
                };
            }
            self.expect("<")?;
            match self.peek()? {
                Some('!') => {
                    self.character()?;
                    match self.peek()? {
                        Some('-') => {
                            self.expect("--")?;
                            self.skip_through("-->")?;
                        }
                        Some('[') => {
                            if self.depth == 0 {
                                return Err(XmlError::Malformed.into());
                            }
                            self.expect("[CDATA[")?;
                            self.text_mode = TextMode::Cdata { brackets: 0 };
                        }
                        _ => self.declaration()?,
                    }
                    continue;
                }
                Some('?') => {
                    self.character()?;
                    self.skip_through("?>")?;
                    continue;
                }
                _ => {}
            }
            let mut quote = None;
            loop {
                let character = self.character()?.ok_or(XmlError::Malformed)?;
                match (quote, character) {
                    (Some(active), value) if active == value => quote = None,
                    (Some(_), _) => {}
                    (None, '\'' | '"') => quote = Some(character),
                    (None, '>') => break,
                    _ => {}
                }
                self.workspace.token.push(character)?;
            }
            self.workspace.token.trim_whitespace();
            if self.workspace.token.as_str().starts_with('/') {
                let name = self.workspace.token.as_str()[1..].trim();
                let depth = self.depth.checked_sub(1).ok_or(XmlError::Malformed)?;
                if name.is_empty()
                    || name.bytes().any(|b| b.is_ascii_whitespace())
                    || self.workspace.open[depth].as_str() != name
                {
                    return Err(XmlError::Malformed.into());
                }
                self.depth = depth;
                if depth == 0 {
                    self.phase = DocumentPhase::Epilog;
                }
                return Ok(Some(XmlEvent::End(local_name(name))));
            }
            let empty = self.workspace.token.as_str().ends_with('/');
            if empty {
                self.workspace.token.length -= 1;
                self.workspace.token.trim_whitespace();
            }
            let source = self.workspace.token.as_str();
            let name_end = source
                .bytes()
                .position(|b| b.is_ascii_whitespace())
                .unwrap_or(source.len());
            if name_end == 0 {
                return Err(XmlError::Malformed.into());
            }
            let name = FixedString::try_from_str(&source[..name_end])?;
            if self.depth == 0 {
                if self.phase == DocumentPhase::Epilog {
                    return Err(XmlError::Malformed.into());
                }
                self.phase = if empty {
                    DocumentPhase::Epilog
                } else {
                    DocumentPhase::Root
                };
            }
            if empty {
                self.pending_end = Some(name);
            } else {
                if self.depth == MAX_XML_DEPTH {
                    return Err(XmlError::NestingTooDeep.into());
                }
                self.workspace.open[self.depth] = name;
                self.depth += 1;
            }
            return Ok(Some(XmlEvent::Start(XmlTag { source, name_end })));
        }
    }
}

#[cfg(test)]
mod tests;
