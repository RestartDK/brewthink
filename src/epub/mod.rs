use std::{
    borrow::Cow,
    collections::HashSet,
    format,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
    string::{String, ToString},
    vec::Vec,
};

use quick_xml::{Reader, events::Event};
use zip::ZipArchive;

const MAX_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 2_048;
const MAX_CONTAINER_BYTES: u64 = 256 * 1024;
const MAX_PACKAGE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_RESOURCE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_INFLATION_RATIO: u64 = 200;
const EPUB_MIMETYPE: &[u8] = b"application/epub+zip";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookMetadata {
    title: String,
    creators: Vec<String>,
    language: Option<String>,
}

impl BookMetadata {
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn creators(&self) -> &[String] {
        &self.creators
    }

    pub fn primary_creator(&self) -> Option<&str> {
        self.creators.first().map(String::as_str)
    }

    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resource {
    id: String,
    path: String,
    media_type: String,
    properties: Vec<String>,
}

impl Resource {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    pub fn has_property(&self, property: &str) -> bool {
        self.properties
            .iter()
            .any(|candidate| candidate == property)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpineItem {
    resource: Resource,
    linear: bool,
}

impl SpineItem {
    pub fn resource(&self) -> &Resource {
        &self.resource
    }

    pub const fn is_linear(&self) -> bool {
        self.linear
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Publication {
    version: String,
    metadata: BookMetadata,
    resources: Vec<Resource>,
    spine: Vec<SpineItem>,
    cover_index: Option<usize>,
    navigation_index: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentStyle {
    Body,
    Heading,
    Quote,
    ListItem,
    Preformatted,
    Caption,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentBlock {
    style: ContentStyle,
    text: String,
}

impl ContentBlock {
    pub const fn style(&self) -> ContentStyle {
        self.style
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChapterContent {
    title: Option<String>,
    blocks: Vec<ContentBlock>,
}

impl ChapterContent {
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn blocks(&self) -> &[ContentBlock] {
        &self.blocks
    }
}

impl Publication {
    pub fn version(&self) -> &str {
        &self.version
    }

    pub const fn metadata(&self) -> &BookMetadata {
        &self.metadata
    }

    pub fn resources(&self) -> &[Resource] {
        &self.resources
    }

    pub fn spine(&self) -> &[SpineItem] {
        &self.spine
    }

    pub fn cover(&self) -> Option<&Resource> {
        self.cover_index.map(|index| &self.resources[index])
    }

    pub fn navigation(&self) -> Option<&Resource> {
        self.navigation_index.map(|index| &self.resources[index])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EpubError {
    ArchiveTooLarge,
    InvalidZip,
    TooManyEntries,
    UnsafePath,
    DuplicateArchivePath,
    EncryptedEntry,
    MissingMimetype,
    MimetypeNotFirst,
    CompressedMimetype,
    InvalidMimetype,
    MissingContainer,
    InvalidContainer,
    MissingPackage,
    InvalidPackage,
    MissingTitle,
    MissingSpine,
    MissingSpineResource,
    DuplicateResourceId,
    ResourceTooLarge,
    ExcessiveInflation,
    ResourceRead,
    UnsupportedTextEncoding,
    SpineOutOfBounds,
    UnsupportedSpineMediaType,
    InvalidContent,
    TooManyContentBlocks,
}

pub struct EpubBook<'a> {
    archive: ZipArchive<Cursor<&'a [u8]>>,
    publication: Publication,
}

impl<'a> EpubBook<'a> {
    pub fn open(encoded: &'a [u8]) -> Result<Self, EpubError> {
        if encoded.len() > MAX_ARCHIVE_BYTES {
            return Err(EpubError::ArchiveTooLarge);
        }

        let mut archive =
            ZipArchive::new(Cursor::new(encoded)).map_err(|_| EpubError::InvalidZip)?;
        inspect_archive(&mut archive)?;

        let mimetype = read_entry(&mut archive, "mimetype", EPUB_MIMETYPE.len() as u64)?
            .ok_or(EpubError::MissingMimetype)?;
        if mimetype != EPUB_MIMETYPE {
            return Err(EpubError::InvalidMimetype);
        }

        let container = read_entry(&mut archive, "META-INF/container.xml", MAX_CONTAINER_BYTES)?
            .ok_or(EpubError::MissingContainer)?;
        let package_path = parse_container(&container)?;
        let package = read_entry(&mut archive, &package_path, MAX_PACKAGE_BYTES)?
            .ok_or(EpubError::MissingPackage)?;
        let publication = parse_package(&package_path, &package)?;

        Ok(Self {
            archive,
            publication,
        })
    }

    pub const fn publication(&self) -> &Publication {
        &self.publication
    }

    pub fn read_resource(&mut self, resource: &Resource) -> Result<Vec<u8>, EpubError> {
        read_entry(&mut self.archive, resource.path(), MAX_RESOURCE_BYTES)?
            .ok_or(EpubError::ResourceRead)
    }

    pub fn read_cover(&mut self) -> Result<Option<Vec<u8>>, EpubError> {
        let Some(index) = self.publication.cover_index else {
            return Ok(None);
        };
        let path = self.publication.resources[index].path.clone();
        read_entry(&mut self.archive, &path, MAX_RESOURCE_BYTES)
    }

    pub fn read_spine_document(&mut self, index: usize) -> Result<ChapterContent, EpubError> {
        let resource = self
            .publication
            .spine
            .get(index)
            .ok_or(EpubError::SpineOutOfBounds)?
            .resource
            .clone();
        if resource.media_type != "application/xhtml+xml" {
            return Err(EpubError::UnsupportedSpineMediaType);
        }
        let encoded = self.read_resource(&resource)?;
        parse_xhtml(&encoded)
    }
}

fn inspect_archive(archive: &mut ZipArchive<Cursor<&[u8]>>) -> Result<(), EpubError> {
    if archive.len() > MAX_ENTRIES {
        return Err(EpubError::TooManyEntries);
    }

    let mut paths = HashSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|_| EpubError::InvalidZip)?;
        if index == 0 {
            if entry.name() != "mimetype" {
                return Err(EpubError::MimetypeNotFirst);
            }
            if entry.compression() != zip::CompressionMethod::Stored {
                return Err(EpubError::CompressedMimetype);
            }
        }
        if entry.encrypted() {
            return Err(EpubError::EncryptedEntry);
        }
        if !safe_archive_path(entry.name()) {
            return Err(EpubError::UnsafePath);
        }
        if !paths.insert(entry.name().to_string()) {
            return Err(EpubError::DuplicateArchivePath);
        }
    }
    Ok(())
}

fn safe_archive_path(value: &str) -> bool {
    if value.is_empty() || value.contains(['\\', '\0']) {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn read_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    maximum_size: u64,
) -> Result<Option<Vec<u8>>, EpubError> {
    let mut entry = match archive.by_name(path) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(_) => return Err(EpubError::ResourceRead),
    };
    if entry.size() > maximum_size {
        return Err(EpubError::ResourceTooLarge);
    }
    if entry.compressed_size() > 0 && entry.size() / entry.compressed_size() > MAX_INFLATION_RATIO {
        return Err(EpubError::ExcessiveInflation);
    }

    let capacity = usize::try_from(entry.size()).map_err(|_| EpubError::ResourceTooLarge)?;
    let mut output = Vec::with_capacity(capacity);
    entry
        .by_ref()
        .take(maximum_size + 1)
        .read_to_end(&mut output)
        .map_err(|_| EpubError::ResourceRead)?;
    if output.len() as u64 > maximum_size || output.len() as u64 != entry.size() {
        return Err(EpubError::ResourceRead);
    }
    Ok(Some(output))
}

fn parse_container(encoded: &[u8]) -> Result<String, EpubError> {
    let mut reader = Reader::from_reader(encoded);
    reader.config_mut().trim_text(true);

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element))
                if element.local_name().as_ref() == b"rootfile" =>
            {
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|_| EpubError::InvalidContainer)?;
                    if attribute.key.local_name().as_ref() == b"full-path" {
                        let value = attribute
                            .decode_and_unescape_value(reader.decoder())
                            .map_err(|_| EpubError::InvalidContainer)?;
                        if safe_archive_path(&value) {
                            return Ok(value.into_owned());
                        }
                        return Err(EpubError::UnsafePath);
                    }
                }
            }
            Ok(Event::Eof) => return Err(EpubError::InvalidContainer),
            Ok(_) => {}
            Err(_) => return Err(EpubError::InvalidContainer),
        }
    }
}

#[derive(Default)]
struct PackageDraft {
    version: String,
    title: Option<String>,
    creators: Vec<String>,
    language: Option<String>,
    resources: Vec<Resource>,
    spine_ids: Vec<(String, bool)>,
    epub2_cover_id: Option<String>,
}

#[derive(Clone, Copy)]
enum MetadataField {
    Title,
    Creator(usize),
    Language,
}

fn parse_package(package_path: &str, encoded: &[u8]) -> Result<Publication, EpubError> {
    let mut reader = Reader::from_reader(encoded);
    let mut draft = PackageDraft::default();
    let mut in_metadata = false;
    let mut metadata_field = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = element.local_name();
                match name.as_ref() {
                    b"package" => {
                        if let Some(value) = attribute_value(&reader, &element, b"version")? {
                            draft.version = value;
                        }
                    }
                    b"metadata" => in_metadata = true,
                    b"title" if in_metadata && draft.title.is_none() => {
                        draft.title = Some(String::new());
                        metadata_field = Some(MetadataField::Title);
                    }
                    b"creator" if in_metadata => {
                        let index = draft.creators.len();
                        draft.creators.push(String::new());
                        metadata_field = Some(MetadataField::Creator(index));
                    }
                    b"language" if in_metadata && draft.language.is_none() => {
                        draft.language = Some(String::new());
                        metadata_field = Some(MetadataField::Language);
                    }
                    b"item" => add_manifest_item(package_path, &reader, &element, &mut draft)?,
                    b"itemref" => add_spine_item(&reader, &element, &mut draft)?,
                    b"meta" if in_metadata => add_legacy_cover(&reader, &element, &mut draft)?,
                    _ => {}
                }
            }
            Ok(Event::Empty(element)) => match element.local_name().as_ref() {
                b"item" => add_manifest_item(package_path, &reader, &element, &mut draft)?,
                b"itemref" => add_spine_item(&reader, &element, &mut draft)?,
                b"meta" if in_metadata => add_legacy_cover(&reader, &element, &mut draft)?,
                _ => {}
            },
            Ok(Event::Text(text)) => {
                if let Some(field) = metadata_field {
                    let value = text
                        .decode()
                        .map_err(|_| EpubError::UnsupportedTextEncoding)?;
                    append_metadata_text(&mut draft, field, &value);
                }
            }
            Ok(Event::CData(text)) => {
                if let Some(field) = metadata_field {
                    let value = text
                        .decode()
                        .map_err(|_| EpubError::UnsupportedTextEncoding)?;
                    append_metadata_text(&mut draft, field, &value);
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(field) = metadata_field {
                    let value = reference
                        .decode()
                        .map_err(|_| EpubError::UnsupportedTextEncoding)?;
                    let value = resolve_reference(&value).ok_or(EpubError::InvalidPackage)?;
                    append_metadata_text(&mut draft, field, &value);
                }
            }
            Ok(Event::End(element)) => match element.local_name().as_ref() {
                b"metadata" => {
                    in_metadata = false;
                    metadata_field = None;
                }
                b"title" | b"creator" | b"language" => metadata_field = None,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(EpubError::InvalidPackage),
        }
    }

    finish_package(draft)
}

fn attribute_value(
    reader: &Reader<&[u8]>,
    element: &quick_xml::events::BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, EpubError> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|_| EpubError::InvalidPackage)?;
        if attribute.key.local_name().as_ref() == name {
            return attribute
                .decode_and_unescape_value(reader.decoder())
                .map(Cow::into_owned)
                .map(Some)
                .map_err(|_| EpubError::InvalidPackage);
        }
    }
    Ok(None)
}

fn add_manifest_item(
    package_path: &str,
    reader: &Reader<&[u8]>,
    element: &quick_xml::events::BytesStart<'_>,
    draft: &mut PackageDraft,
) -> Result<(), EpubError> {
    let id = attribute_value(reader, element, b"id")?.ok_or(EpubError::InvalidPackage)?;
    let href = attribute_value(reader, element, b"href")?.ok_or(EpubError::InvalidPackage)?;
    let media_type =
        attribute_value(reader, element, b"media-type")?.ok_or(EpubError::InvalidPackage)?;
    let properties = attribute_value(reader, element, b"properties")?
        .unwrap_or_default()
        .split_ascii_whitespace()
        .map(ToString::to_string)
        .collect();
    let path = resolve_resource_path(package_path, &href)?;
    draft.resources.push(Resource {
        id,
        path,
        media_type,
        properties,
    });
    Ok(())
}

fn add_spine_item(
    reader: &Reader<&[u8]>,
    element: &quick_xml::events::BytesStart<'_>,
    draft: &mut PackageDraft,
) -> Result<(), EpubError> {
    let id = attribute_value(reader, element, b"idref")?.ok_or(EpubError::InvalidPackage)?;
    let linear = attribute_value(reader, element, b"linear")?.as_deref() != Some("no");
    draft.spine_ids.push((id, linear));
    Ok(())
}

fn add_legacy_cover(
    reader: &Reader<&[u8]>,
    element: &quick_xml::events::BytesStart<'_>,
    draft: &mut PackageDraft,
) -> Result<(), EpubError> {
    if attribute_value(reader, element, b"name")?.as_deref() == Some("cover") {
        draft.epub2_cover_id = attribute_value(reader, element, b"content")?;
    }
    Ok(())
}

fn resolve_resource_path(package_path: &str, href: &str) -> Result<String, EpubError> {
    let href = href.split(['#', '?']).next().unwrap_or_default();
    let href = decode_href(href)?;
    if href.is_empty() || href.contains(['\\', '\0']) {
        return Err(EpubError::InvalidPackage);
    }
    let base = Path::new(package_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let joined = base.join(href);
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            Component::ParentDir if normalized.pop() => {}
            _ => return Err(EpubError::UnsafePath),
        }
    }
    normalized
        .to_str()
        .map(|path| path.replace('\\', "/"))
        .ok_or(EpubError::InvalidPackage)
}

fn decode_href(href: &str) -> Result<String, EpubError> {
    let input = href.as_bytes();
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let high = *input.get(index + 1).ok_or(EpubError::InvalidPackage)?;
            let low = *input.get(index + 2).ok_or(EpubError::InvalidPackage)?;
            output.push(
                hex_value(high)
                    .and_then(|high| hex_value(low).map(|low| high * 16 + low))
                    .ok_or(EpubError::InvalidPackage)?,
            );
            index += 3;
        } else {
            output.push(input[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| EpubError::InvalidPackage)
}

const fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn append_metadata_text(draft: &mut PackageDraft, field: MetadataField, value: &str) {
    match field {
        MetadataField::Title => draft.title.as_mut().unwrap().push_str(value),
        MetadataField::Creator(index) => draft.creators[index].push_str(value),
        MetadataField::Language => draft.language.as_mut().unwrap().push_str(value),
    }
}

fn resolve_reference(value: &str) -> Option<String> {
    if let Some(decimal) = value.strip_prefix('#') {
        let codepoint = if let Some(hexadecimal) = decimal
            .strip_prefix('x')
            .or_else(|| decimal.strip_prefix('X'))
        {
            u32::from_str_radix(hexadecimal, 16).ok()?
        } else {
            decimal.parse().ok()?
        };
        return char::from_u32(codepoint).map(|character| character.to_string());
    }
    quick_xml::escape::resolve_xml_entity(value).map(ToString::to_string)
}

fn normalized_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn finish_package(mut draft: PackageDraft) -> Result<Publication, EpubError> {
    if draft.version.is_empty() {
        return Err(EpubError::InvalidPackage);
    }
    let title = normalized_text(draft.title.as_deref().ok_or(EpubError::MissingTitle)?);
    if title.is_empty() {
        return Err(EpubError::MissingTitle);
    }
    draft.creators = draft
        .creators
        .iter()
        .map(|creator| normalized_text(creator))
        .filter(|creator| !creator.is_empty())
        .collect();
    draft.language = draft
        .language
        .as_deref()
        .map(normalized_text)
        .filter(|language| !language.is_empty());
    let mut resource_ids = HashSet::new();
    if draft
        .resources
        .iter()
        .any(|resource| !resource_ids.insert(resource.id.as_str()))
    {
        return Err(EpubError::DuplicateResourceId);
    }
    let cover_index = draft
        .resources
        .iter()
        .position(|resource| resource.has_property("cover-image"))
        .or_else(|| {
            draft.epub2_cover_id.as_deref().and_then(|id| {
                draft
                    .resources
                    .iter()
                    .position(|resource| resource.id == id)
            })
        });
    let navigation_index = draft
        .resources
        .iter()
        .position(|resource| resource.has_property("nav"))
        .or_else(|| {
            draft
                .resources
                .iter()
                .position(|resource| resource.media_type == "application/x-dtbncx+xml")
        });
    if draft.spine_ids.is_empty() {
        return Err(EpubError::MissingSpine);
    }
    let mut spine = Vec::with_capacity(draft.spine_ids.len());
    for (id, linear) in draft.spine_ids {
        let resource = draft
            .resources
            .iter()
            .find(|resource| resource.id == id)
            .cloned()
            .ok_or(EpubError::MissingSpineResource)?;
        spine.push(SpineItem { resource, linear });
    }

    Ok(Publication {
        version: draft.version,
        metadata: BookMetadata {
            title,
            creators: draft.creators,
            language: draft.language,
        },
        resources: draft.resources,
        spine,
        cover_index,
        navigation_index,
    })
}

const MAX_CONTENT_BLOCKS: usize = 16_384;

#[derive(Default)]
struct ContentDraft {
    title: Option<String>,
    blocks: Vec<ContentBlock>,
    current: String,
    current_style: Option<ContentStyle>,
    quote_depth: usize,
    list_depth: usize,
    hidden_depth: usize,
    in_body: bool,
}

impl ContentDraft {
    fn begin(&mut self, style: ContentStyle) -> Result<(), EpubError> {
        self.finish()?;
        self.current_style = Some(style);
        if style == ContentStyle::ListItem {
            self.current.push_str("• ");
        }
        Ok(())
    }

    fn append(&mut self, value: &str) {
        if self.current_style.is_none() {
            self.current_style = Some(self.contextual_style());
        }
        self.current.push_str(value);
    }

    fn finish(&mut self) -> Result<(), EpubError> {
        let style = self.current_style.take().unwrap_or(ContentStyle::Body);
        let text = normalized_text(&self.current);
        self.current.clear();
        if text.is_empty() {
            return Ok(());
        }
        if self.blocks.len() >= MAX_CONTENT_BLOCKS {
            return Err(EpubError::TooManyContentBlocks);
        }
        if style == ContentStyle::Heading && self.title.is_none() {
            self.title = Some(text.clone());
        }
        self.blocks.push(ContentBlock { style, text });
        Ok(())
    }

    const fn contextual_style(&self) -> ContentStyle {
        if self.list_depth > 0 {
            ContentStyle::ListItem
        } else if self.quote_depth > 0 {
            ContentStyle::Quote
        } else {
            ContentStyle::Body
        }
    }
}

fn parse_xhtml(encoded: &[u8]) -> Result<ChapterContent, EpubError> {
    let mut reader = Reader::from_reader(encoded);
    let mut draft = ContentDraft::default();

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = element.local_name();
                let name = name.as_ref();
                if draft.hidden_depth > 0 {
                    draft.hidden_depth += 1;
                    continue;
                }
                if name == b"body" {
                    draft.in_body = true;
                    continue;
                }
                if !draft.in_body {
                    continue;
                }
                if matches!(name, b"script" | b"style") {
                    draft.hidden_depth = 1;
                    continue;
                }
                match name {
                    b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" => {
                        draft.begin(ContentStyle::Heading)?
                    }
                    b"blockquote" => {
                        draft.quote_depth += 1;
                        draft.begin(ContentStyle::Quote)?;
                    }
                    b"li" => {
                        draft.list_depth += 1;
                        draft.begin(ContentStyle::ListItem)?;
                    }
                    b"p" => draft.begin(draft.contextual_style())?,
                    b"pre" => draft.begin(ContentStyle::Preformatted)?,
                    b"figcaption" | b"caption" => draft.begin(ContentStyle::Caption)?,
                    b"tr" => draft.begin(ContentStyle::Body)?,
                    b"td" | b"th" if !draft.current.is_empty() => draft.current.push_str(" | "),
                    b"br" => draft.append("\n"),
                    b"img" => append_image_alternative(&reader, &element, &mut draft)?,
                    b"hr" => {
                        draft.finish()?;
                        draft.current_style = Some(ContentStyle::Body);
                        draft.current.push_str("────────────────────────");
                        draft.finish()?;
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(element)) => {
                if draft.hidden_depth > 0 || !draft.in_body {
                    continue;
                }
                match element.local_name().as_ref() {
                    b"br" => draft.append("\n"),
                    b"img" => append_image_alternative(&reader, &element, &mut draft)?,
                    b"hr" => {
                        draft.finish()?;
                        draft.current_style = Some(ContentStyle::Body);
                        draft.current.push_str("────────────────────────");
                        draft.finish()?;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(text)) if draft.in_body && draft.hidden_depth == 0 => {
                let value = text
                    .decode()
                    .map_err(|_| EpubError::UnsupportedTextEncoding)?;
                draft.append(&value);
            }
            Ok(Event::CData(text)) if draft.in_body && draft.hidden_depth == 0 => {
                let value = text
                    .decode()
                    .map_err(|_| EpubError::UnsupportedTextEncoding)?;
                draft.append(&value);
            }
            Ok(Event::GeneralRef(reference)) if draft.in_body && draft.hidden_depth == 0 => {
                let value = reference
                    .decode()
                    .map_err(|_| EpubError::UnsupportedTextEncoding)?;
                let value = resolve_reference(&value).ok_or(EpubError::InvalidContent)?;
                draft.append(&value);
            }
            Ok(Event::End(element)) => {
                if draft.hidden_depth > 0 {
                    draft.hidden_depth -= 1;
                    continue;
                }
                let name = element.local_name();
                let name = name.as_ref();
                if name == b"body" {
                    draft.finish()?;
                    draft.in_body = false;
                    continue;
                }
                if !draft.in_body {
                    continue;
                }
                match name {
                    b"h1" | b"h2" | b"h3" | b"h4" | b"h5" | b"h6" | b"p" | b"pre"
                    | b"figcaption" | b"caption" | b"tr" => draft.finish()?,
                    b"li" => {
                        draft.finish()?;
                        draft.list_depth = draft.list_depth.saturating_sub(1);
                    }
                    b"blockquote" => {
                        draft.finish()?;
                        draft.quote_depth = draft.quote_depth.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(EpubError::InvalidContent),
        }
    }
    draft.finish()?;
    if draft.blocks.is_empty() {
        draft.blocks.push(ContentBlock {
            style: ContentStyle::Body,
            text: "This chapter contains no readable text.".to_string(),
        });
    }
    Ok(ChapterContent {
        title: draft.title,
        blocks: draft.blocks,
    })
}

fn append_image_alternative(
    reader: &Reader<&[u8]>,
    element: &quick_xml::events::BytesStart<'_>,
    draft: &mut ContentDraft,
) -> Result<(), EpubError> {
    let alternative = content_attribute_value(reader, element, b"alt")?
        .filter(|value| !value.trim().is_empty())
        .map_or_else(
            || "[Image]".to_string(),
            |value| format!("[Image: {value}]"),
        );
    if !draft.current.is_empty() {
        draft.current.push(' ');
    }
    draft.append(&alternative);
    Ok(())
}

fn content_attribute_value(
    reader: &Reader<&[u8]>,
    element: &quick_xml::events::BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, EpubError> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|_| EpubError::InvalidContent)?;
        if attribute.key.local_name().as_ref() == name {
            return attribute
                .decode_and_unescape_value(reader.decoder())
                .map(Cow::into_owned)
                .map(Some)
                .map_err(|_| EpubError::InvalidContent);
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Cursor, Write},
        vec::Vec,
    };

    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::{ContentStyle, EpubBook, EpubError};

    const CONTAINER: &str = r#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0">
  <rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
    const PACKAGE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Small &amp; Typed</dc:title>
    <dc:creator>Daniel Reader</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="cover" href="images/%63over.png" media-type="image/png" properties="cover-image"/>
    <item id="chapter" href="text/chapter.xhtml" media-type="application/xhtml+xml"/>
    <item id="nav" href="text/nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
  </manifest>
  <spine><itemref idref="chapter"/></spine>
</package>"#;

    fn epub(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut encoded = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut encoded);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for (name, bytes) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer.finish().unwrap();
        }
        encoded.into_inner()
    }

    fn valid_epub() -> Vec<u8> {
        epub(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OPS/book.opf", PACKAGE.as_bytes()),
            ("OPS/images/cover.png", b"cover"),
            ("OPS/text/chapter.xhtml", b"<html/>"),
        ])
    }

    #[test]
    fn parses_metadata_manifest_spine_and_cover() {
        let encoded = valid_epub();
        let mut book = EpubBook::open(&encoded).unwrap();
        let publication = book.publication();

        assert_eq!(publication.version(), "3.0");
        assert_eq!(publication.metadata().title(), "Small & Typed");
        assert_eq!(
            publication.metadata().primary_creator(),
            Some("Daniel Reader")
        );
        assert_eq!(publication.metadata().language(), Some("en"));
        assert_eq!(publication.spine().len(), 1);
        assert_eq!(
            publication.spine()[0].resource().path(),
            "OPS/text/chapter.xhtml"
        );
        assert_eq!(publication.cover().unwrap().path(), "OPS/images/cover.png");
        assert_eq!(
            publication.navigation().unwrap().path(),
            "OPS/text/nav.xhtml"
        );
        assert!(publication.spine()[0].is_linear());
        assert_eq!(book.read_cover().unwrap().unwrap(), b"cover");
    }

    #[test]
    fn parses_spine_xhtml_without_dropping_unknown_text() {
        let chapter = br#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body>
<h1>Opening &amp; Context</h1>
<p>Words with <em>emphasis</em> and <unknown>future markup</unknown>.</p>
<ul><li>First item</li><li>Second item</li></ul>
<blockquote><p>A quotation.</p></blockquote>
<figure><img src="diagram.png" alt="A useful diagram"/><figcaption>Figure one</figcaption></figure>
<table><tr><th>Kind</th><th>Value</th></tr><tr><td>A</td><td>42</td></tr></table>
<script>discardExecutableText()</script>
</body></html>"#;
        let encoded = epub(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OPS/book.opf", PACKAGE.as_bytes()),
            ("OPS/text/chapter.xhtml", chapter),
        ]);
        let mut book = EpubBook::open(&encoded).unwrap();

        let content = book.read_spine_document(0).unwrap();
        assert_eq!(content.title(), Some("Opening & Context"));
        assert!(content.blocks().iter().any(|block| {
            block.style() == ContentStyle::Body
                && block.text() == "Words with emphasis and future markup."
        }));
        assert!(content.blocks().iter().any(|block| {
            block.style() == ContentStyle::ListItem && block.text() == "• First item"
        }));
        assert!(content.blocks().iter().any(|block| {
            block.style() == ContentStyle::Quote && block.text() == "A quotation."
        }));
        assert!(
            content
                .blocks()
                .iter()
                .any(|block| block.text() == "[Image: A useful diagram]")
        );
        assert!(
            content
                .blocks()
                .iter()
                .any(|block| block.text() == "Kind | Value")
        );
        assert!(
            content
                .blocks()
                .iter()
                .all(|block| !block.text().contains("discardExecutableText"))
        );
    }

    #[test]
    fn discovers_epub2_ncx_navigation() {
        let package = PACKAGE.replace(
            "application/xhtml+xml\" properties=\"nav\"",
            "application/x-dtbncx+xml\"",
        );
        let encoded = epub(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OPS/book.opf", package.as_bytes()),
        ]);
        let book = EpubBook::open(&encoded).unwrap();

        assert_eq!(
            book.publication().navigation().unwrap().path(),
            "OPS/text/nav.xhtml"
        );
    }

    #[test]
    fn rejects_paths_that_escape_the_archive() {
        let encoded = epub(&[
            ("mimetype", b"application/epub+zip"),
            ("../book.opf", b"bad"),
        ]);
        assert!(matches!(
            EpubBook::open(&encoded),
            Err(EpubError::UnsafePath)
        ));
    }

    #[test]
    fn rejects_a_missing_epub_mimetype() {
        let encoded = epub(&[]);
        assert!(matches!(
            EpubBook::open(&encoded),
            Err(EpubError::MissingMimetype)
        ));
    }

    #[test]
    fn requires_the_mimetype_to_be_the_first_entry() {
        let encoded = epub(&[
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("mimetype", b"application/epub+zip"),
        ]);
        assert!(matches!(
            EpubBook::open(&encoded),
            Err(EpubError::MimetypeNotFirst)
        ));
    }

    #[test]
    fn rejects_duplicate_manifest_identifiers() {
        let package = PACKAGE.replace("id=\"chapter\"", "id=\"cover\"");
        let encoded = epub(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OPS/book.opf", package.as_bytes()),
        ]);
        assert!(matches!(
            EpubBook::open(&encoded),
            Err(EpubError::DuplicateResourceId)
        ));
    }
}
