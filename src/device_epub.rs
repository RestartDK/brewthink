use crate::{
    bounded_xml::{FixedString, XmlError, XmlEvent, XmlReader},
    zip_stream::{InflateWorkspace, ReadAt, StreamingZip, ZipError, ZipValidationScratch},
};

pub const MAX_DEVICE_SPINE_ITEMS: usize = 64;
pub const MAX_DEVICE_MANIFEST_ITEMS: usize = 512;
pub const MAX_CONTAINER_BYTES: usize = 2 * 1024;
pub const MAX_PACKAGE_BYTES: usize = 64 * 1024;
pub const MAX_DEVICE_RESOURCE_BYTES: usize = 140 * 1024;
pub const MAX_DEVICE_PATH_BYTES: usize = 128;

const EPUB_MIMETYPE: &[u8] = b"application/epub+zip";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceSpineItem {
    path: FixedString<MAX_DEVICE_PATH_BYTES>,
}

impl DeviceSpineItem {
    pub fn path(&self) -> &str {
        self.path.as_str()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DevicePublication {
    title: FixedString<192>,
    creator: FixedString<128>,
    spine: [Option<DeviceSpineItem>; MAX_DEVICE_SPINE_ITEMS],
    spine_length: u8,
    cover: Option<FixedString<MAX_DEVICE_PATH_BYTES>>,
}

impl DevicePublication {
    pub fn title(&self) -> &str {
        self.title.as_str()
    }

    pub fn creator(&self) -> &str {
        self.creator.as_str()
    }

    pub const fn spine_len(&self) -> usize {
        self.spine_length as usize
    }

    pub fn spine_item(&self, index: usize) -> Option<&DeviceSpineItem> {
        self.spine.get(index).and_then(Option::as_ref)
    }

    pub fn cover_path(&self) -> Option<&str> {
        self.cover.as_ref().map(FixedString::as_str)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum DeviceEpubError<E> {
    Zip(ZipError<E>),
    Xml(XmlError),
    MissingMimetype,
    InvalidMimetype,
    MimetypeNotFirst,
    CompressedMimetype,
    ContainerTooLarge,
    MissingContainer,
    InvalidContainer,
    PackageTooLarge,
    MissingPackage,
    InvalidPackage,
    MissingTitle,
    MissingSpine,
    TooManySpineItems,
    TooManyManifestItems,
    DuplicateManifestId,
    MissingSpineResource,
    UnsupportedSpineMediaType,
    ResourceTooLarge,
    SpineOutOfBounds,
}

impl<E> From<XmlError> for DeviceEpubError<E> {
    fn from(error: XmlError) -> Self {
        Self::Xml(error)
    }
}

pub struct DevicePackageScratch {
    spine_ids: [Option<FixedString<48>>; MAX_DEVICE_SPINE_ITEMS],
    manifest_hashes: [u32; MAX_DEVICE_MANIFEST_ITEMS],
    manifest_length: usize,
    legacy_cover_id: Option<FixedString<48>>,
}

impl DevicePackageScratch {
    pub const fn new() -> Self {
        Self {
            spine_ids: [None; MAX_DEVICE_SPINE_ITEMS],
            manifest_hashes: [0; MAX_DEVICE_MANIFEST_ITEMS],
            manifest_length: 0,
            legacy_cover_id: None,
        }
    }

    fn reset(&mut self) {
        self.spine_ids.fill(None);
        self.manifest_length = 0;
        self.legacy_cover_id = None;
    }

    fn insert_manifest_id(
        &mut self,
        id: &str,
    ) -> Result<(), DeviceEpubError<core::convert::Infallible>> {
        if self.manifest_length == MAX_DEVICE_MANIFEST_ITEMS {
            return Err(DeviceEpubError::TooManyManifestItems);
        }
        let hash = path_hash(id.as_bytes());
        if self.manifest_hashes[..self.manifest_length].contains(&hash) {
            return Err(DeviceEpubError::DuplicateManifestId);
        }
        self.manifest_hashes[self.manifest_length] = hash;
        self.manifest_length += 1;
        Ok(())
    }
}

impl Default for DevicePackageScratch {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DeviceEpub<R> {
    archive: StreamingZip<R>,
    publication: DevicePublication,
}

impl<R> DeviceEpub<R>
where
    R: ReadAt,
{
    #[inline(always)]
    pub fn open(
        reader: R,
        zip_scratch: &mut ZipValidationScratch,
        package_scratch: &mut DevicePackageScratch,
        inflater: &mut InflateWorkspace,
        resource_buffer: &mut [u8; MAX_DEVICE_RESOURCE_BYTES],
    ) -> Result<Self, DeviceEpubError<R::Error>> {
        let archive = StreamingZip::open(reader, zip_scratch).map_err(DeviceEpubError::Zip)?;
        let first = archive.first_entry().map_err(DeviceEpubError::Zip)?;
        if first.path().as_str() != "mimetype" {
            return Err(DeviceEpubError::MimetypeNotFirst);
        }
        if !first.is_stored() {
            return Err(DeviceEpubError::CompressedMimetype);
        }
        if first.uncompressed_size() as usize != EPUB_MIMETYPE.len() {
            return Err(DeviceEpubError::InvalidMimetype);
        }
        let mut mimetype = [0; EPUB_MIMETYPE.len()];
        archive
            .read_entry(first, &mut mimetype, inflater)
            .map_err(DeviceEpubError::Zip)?;
        if mimetype != EPUB_MIMETYPE {
            return Err(DeviceEpubError::InvalidMimetype);
        }

        let container = archive
            .find("META-INF/container.xml")
            .map_err(|error| match error {
                ZipError::EntryNotFound => DeviceEpubError::MissingContainer,
                other => DeviceEpubError::Zip(other),
            })?;
        if container.uncompressed_size() as usize > MAX_CONTAINER_BYTES {
            return Err(DeviceEpubError::ContainerTooLarge);
        }
        let container_length = archive
            .read_entry(container, resource_buffer, inflater)
            .map_err(DeviceEpubError::Zip)?;
        let package_path = parse_container(&resource_buffer[..container_length])?;
        let package_entry = archive
            .find(package_path.as_str())
            .map_err(|error| match error {
                ZipError::EntryNotFound => DeviceEpubError::MissingPackage,
                other => DeviceEpubError::Zip(other),
            })?;
        if package_entry.uncompressed_size() as usize > MAX_PACKAGE_BYTES {
            return Err(DeviceEpubError::PackageTooLarge);
        }
        let package_length = archive
            .read_entry(package_entry, resource_buffer, inflater)
            .map_err(DeviceEpubError::Zip)?;
        let publication = parse_package(
            package_path.as_str(),
            &resource_buffer[..package_length],
            package_scratch,
        )?;
        Ok(Self {
            archive,
            publication,
        })
    }

    pub const fn publication(&self) -> &DevicePublication {
        &self.publication
    }

    pub fn read_spine(
        &self,
        index: usize,
        output: &mut [u8],
        inflater: &mut InflateWorkspace,
    ) -> Result<usize, DeviceEpubError<R::Error>> {
        let item = self
            .publication
            .spine_item(index)
            .ok_or(DeviceEpubError::SpineOutOfBounds)?;
        self.read_path(item.path(), output, inflater)
    }

    pub fn read_cover(
        &self,
        output: &mut [u8],
        inflater: &mut InflateWorkspace,
    ) -> Result<Option<usize>, DeviceEpubError<R::Error>> {
        self.publication
            .cover_path()
            .map(|path| self.read_path(path, output, inflater))
            .transpose()
    }

    pub fn into_reader(self) -> R {
        self.archive.into_reader()
    }

    fn read_path(
        &self,
        path: &str,
        output: &mut [u8],
        inflater: &mut InflateWorkspace,
    ) -> Result<usize, DeviceEpubError<R::Error>> {
        let entry = self.archive.find(path).map_err(DeviceEpubError::Zip)?;
        if entry.uncompressed_size() as usize > output.len()
            || entry.uncompressed_size() as usize > MAX_DEVICE_RESOURCE_BYTES
        {
            return Err(DeviceEpubError::ResourceTooLarge);
        }
        self.archive
            .read_entry(entry, output, inflater)
            .map_err(DeviceEpubError::Zip)
    }
}

fn parse_container<E>(encoded: &[u8]) -> Result<FixedString<256>, DeviceEpubError<E>> {
    let mut reader = XmlReader::new(encoded)?;
    while let Some(event) = reader.next_event()? {
        if let XmlEvent::Start(tag) = event
            && tag.local_name() == "rootfile"
            && let Some(path) = tag.attribute("full-path")?
        {
            return FixedString::from_decoded(path).map_err(DeviceEpubError::Xml);
        }
    }
    Err(DeviceEpubError::InvalidContainer)
}

#[inline(always)]
fn parse_package<E>(
    package_path: &str,
    encoded: &[u8],
    scratch: &mut DevicePackageScratch,
) -> Result<DevicePublication, DeviceEpubError<E>> {
    scratch.reset();
    let mut publication = DevicePublication {
        title: FixedString::new(),
        creator: FixedString::new(),
        spine: [None; MAX_DEVICE_SPINE_ITEMS],
        spine_length: 0,
        cover: None,
    };
    parse_package_structure(encoded, scratch, &mut publication)?;
    resolve_manifest(package_path, encoded, scratch, &mut publication)?;
    publication.title.normalize_whitespace();
    publication.creator.normalize_whitespace();
    if publication.title.is_empty() {
        return Err(DeviceEpubError::MissingTitle);
    }
    if publication.creator.is_empty() {
        publication.creator.push_str("Unknown author")?;
    }
    if publication.spine_length == 0 {
        return Err(DeviceEpubError::MissingSpine);
    }
    if publication.spine[..usize::from(publication.spine_length)]
        .iter()
        .any(Option::is_none)
    {
        return Err(DeviceEpubError::MissingSpineResource);
    }
    Ok(publication)
}

fn parse_package_structure<E>(
    encoded: &[u8],
    scratch: &mut DevicePackageScratch,
    publication: &mut DevicePublication,
) -> Result<(), DeviceEpubError<E>> {
    let mut reader = XmlReader::new(encoded)?;
    let mut in_metadata = false;
    let mut text_field = None;
    while let Some(event) = reader.next_event()? {
        match event {
            XmlEvent::Start(tag) => match tag.local_name() {
                "metadata" => in_metadata = true,
                "title" if in_metadata && publication.title.is_empty() => text_field = Some(0),
                "creator" if in_metadata && publication.creator.is_empty() => text_field = Some(1),
                "meta" if in_metadata => {
                    if tag.attribute("name")? == Some("cover")
                        && let Some(id) = tag.attribute("content")?
                    {
                        scratch.legacy_cover_id = Some(FixedString::from_decoded(id)?);
                    }
                }
                "itemref" => {
                    if tag.attribute("linear")? == Some("no") {
                        continue;
                    }
                    let id = tag
                        .attribute("idref")?
                        .ok_or(DeviceEpubError::InvalidPackage)?;
                    let index = usize::from(publication.spine_length);
                    if index == MAX_DEVICE_SPINE_ITEMS {
                        return Err(DeviceEpubError::TooManySpineItems);
                    }
                    scratch.spine_ids[index] = Some(FixedString::from_decoded(id)?);
                    publication.spine_length += 1;
                }
                _ => {}
            },
            XmlEvent::Text(text) => match text_field {
                Some(0) => crate::bounded_xml::decode_entities(text, |character| {
                    publication.title.push(character)
                })?,
                Some(1) => crate::bounded_xml::decode_entities(text, |character| {
                    publication.creator.push(character)
                })?,
                _ => {}
            },
            XmlEvent::End(name) => match name {
                "metadata" => {
                    in_metadata = false;
                    text_field = None;
                }
                "title" | "creator" => text_field = None,
                _ => {}
            },
        }
    }
    Ok(())
}

fn resolve_manifest<E>(
    package_path: &str,
    encoded: &[u8],
    scratch: &mut DevicePackageScratch,
    publication: &mut DevicePublication,
) -> Result<(), DeviceEpubError<E>> {
    let mut reader = XmlReader::new(encoded)?;
    while let Some(event) = reader.next_event()? {
        let XmlEvent::Start(tag) = event else {
            continue;
        };
        if tag.local_name() != "item" {
            continue;
        }
        let id = tag
            .attribute("id")?
            .ok_or(DeviceEpubError::InvalidPackage)?;
        scratch.insert_manifest_id(id).map_err(map_infallible)?;
        let href = tag
            .attribute("href")?
            .ok_or(DeviceEpubError::InvalidPackage)?;
        let media_type = tag
            .attribute("media-type")?
            .ok_or(DeviceEpubError::InvalidPackage)?;
        let properties = tag.attribute("properties")?.unwrap_or("");
        let path = resolve_resource_path(package_path, href)?;
        let cover_property = properties
            .split_ascii_whitespace()
            .any(|property| property == "cover-image");
        let legacy_cover = scratch
            .legacy_cover_id
            .as_ref()
            .is_some_and(|cover_id| cover_id.as_str() == id);
        if cover_property || legacy_cover {
            publication.cover = Some(path);
        }
        for index in 0..usize::from(publication.spine_length) {
            if scratch.spine_ids[index]
                .as_ref()
                .is_some_and(|spine_id| spine_id.as_str() == id)
            {
                if media_type != "application/xhtml+xml" {
                    return Err(DeviceEpubError::UnsupportedSpineMediaType);
                }
                if publication.spine[index].is_some() {
                    return Err(DeviceEpubError::DuplicateManifestId);
                }
                publication.spine[index] = Some(DeviceSpineItem { path });
            }
        }
    }
    Ok(())
}

fn resolve_resource_path<E>(
    package_path: &str,
    raw_href: &str,
) -> Result<FixedString<MAX_DEVICE_PATH_BYTES>, DeviceEpubError<E>> {
    let raw_href = raw_href.split(['#', '?']).next().unwrap_or("");
    let decoded_entities = FixedString::<256>::from_decoded(raw_href)?;
    let decoded_href = percent_decode(decoded_entities.as_str())?;
    let href = decoded_href.as_str();
    if href.is_empty() || href.starts_with('/') || href.contains(['\\', '\0']) {
        return Err(DeviceEpubError::InvalidPackage);
    }
    let mut segments: [Option<&str>; 32] = [None; 32];
    let mut length = 0usize;
    if let Some((directory, _)) = package_path.rsplit_once('/') {
        for segment in directory.split('/') {
            push_segment(&mut segments, &mut length, segment)?;
        }
    }
    for segment in href.split('/') {
        match segment {
            "" | "." => {}
            ".." if length > 0 => length -= 1,
            ".." => return Err(DeviceEpubError::InvalidPackage),
            value => push_segment(&mut segments, &mut length, value)?,
        }
    }
    if length == 0 {
        return Err(DeviceEpubError::InvalidPackage);
    }
    let mut output = FixedString::new();
    for (index, segment) in segments[..length].iter().enumerate() {
        if index > 0 {
            output.push('/')?;
        }
        output.push_str(segment.expect("filled path segment"))?;
    }
    Ok(output)
}

fn push_segment<'a, E>(
    segments: &mut [Option<&'a str>; 32],
    length: &mut usize,
    segment: &'a str,
) -> Result<(), DeviceEpubError<E>> {
    if segment.is_empty() || *length == segments.len() {
        return Err(DeviceEpubError::InvalidPackage);
    }
    segments[*length] = Some(segment);
    *length += 1;
    Ok(())
}

fn percent_decode<E>(value: &str) -> Result<FixedString<256>, DeviceEpubError<E>> {
    let bytes = value.as_bytes();
    let mut decoded = [0; 256];
    let mut input = 0;
    let mut output = 0;
    while input < bytes.len() {
        if output == decoded.len() {
            return Err(DeviceEpubError::InvalidPackage);
        }
        if bytes[input] == b'%' {
            let high = *bytes
                .get(input + 1)
                .ok_or(DeviceEpubError::InvalidPackage)?;
            let low = *bytes
                .get(input + 2)
                .ok_or(DeviceEpubError::InvalidPackage)?;
            decoded[output] = hex(high)
                .and_then(|high| hex(low).map(|low| high * 16 + low))
                .ok_or(DeviceEpubError::InvalidPackage)?;
            input += 3;
        } else {
            decoded[output] = bytes[input];
            input += 1;
        }
        output += 1;
    }
    let value =
        core::str::from_utf8(&decoded[..output]).map_err(|_| DeviceEpubError::InvalidPackage)?;
    FixedString::try_from_str(value).map_err(DeviceEpubError::Xml)
}

const fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn map_infallible<E>(error: DeviceEpubError<core::convert::Infallible>) -> DeviceEpubError<E> {
    match error {
        DeviceEpubError::TooManyManifestItems => DeviceEpubError::TooManyManifestItems,
        DeviceEpubError::DuplicateManifestId => DeviceEpubError::DuplicateManifestId,
        _ => unreachable!("manifest scratch only returns count or duplicate errors"),
    }
}

fn path_hash(path: &[u8]) -> u32 {
    path.iter().fold(0x811C_9DC5, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::{boxed::Box, convert::Infallible};

    use super::{DeviceEpub, DevicePackageScratch, resolve_resource_path};
    use crate::zip_stream::{InflateWorkspace, ReadAt, ZipValidationScratch};

    struct SliceFile<'a>(&'a [u8]);

    impl ReadAt for SliceFile<'_> {
        type Error = Infallible;

        fn len(&self) -> u32 {
            self.0.len() as u32
        }

        fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
            let start = offset as usize;
            let count = self.0.len().saturating_sub(start).min(output.len());
            output[..count].copy_from_slice(&self.0[start..start + count]);
            Ok(count)
        }
    }

    #[test]
    fn opens_the_synthetic_epub_with_fixed_memory() {
        let encoded = include_bytes!("../web/tests/fixtures/minimal.epub");
        let mut zip_scratch = Box::new(ZipValidationScratch::new());
        let mut package_scratch = Box::new(DevicePackageScratch::new());
        let mut inflater = Box::new(InflateWorkspace::new());
        let mut resource = Box::new([0; super::MAX_DEVICE_RESOURCE_BYTES]);

        let book = DeviceEpub::open(
            SliceFile(encoded),
            &mut zip_scratch,
            &mut package_scratch,
            &mut inflater,
            &mut resource,
        )
        .unwrap();

        assert_eq!(book.publication().title(), "Synthetic & Safe");
        assert_eq!(book.publication().creator(), "Fixture Author");
        assert_eq!(book.publication().spine_len(), 1);
        assert_eq!(
            book.publication().spine_item(0).unwrap().path(),
            "EPUB/chapter.xhtml"
        );
        assert_eq!(book.publication().cover_path(), Some("EPUB/cover.png"));
    }

    #[test]
    fn resolves_percent_encoded_relative_resources() {
        let path = resolve_resource_path::<Infallible>(
            "OPS/package.opf",
            "Text/../Images/%63over.png#page",
        )
        .unwrap();
        assert_eq!(path.as_str(), "OPS/Images/cover.png");
    }
}
