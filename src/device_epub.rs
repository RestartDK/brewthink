use crate::{
    app::{SeriesMeta, SeriesPosition},
    bounded_xml::{FixedString, XmlError, XmlEvent, XmlReader, XmlText},
    zip_stream::{InflateWorkspace, ReadAt, StreamingZip, ZipError, ZipValidationScratch},
};

pub const MAX_DEVICE_SPINE_ITEMS: usize = 128;
pub const MAX_DEVICE_MANIFEST_ITEMS: usize = 512;
pub const MAX_CONTAINER_BYTES: usize = 2 * 1024;
pub const MAX_PACKAGE_BYTES: usize = 64 * 1024;
pub const MAX_DEVICE_RESOURCE_BYTES: usize = 140 * 1024;
pub const MAX_CHAPTER_BYTES: u32 = 16 * 1024 * 1024;
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

pub const MAX_SERIES_NAME_BYTES: usize = 128;

#[derive(Debug, Eq, PartialEq)]
pub struct DevicePublication {
    title: FixedString<192>,
    creator: FixedString<128>,
    series: FixedString<MAX_SERIES_NAME_BYTES>,
    series_position: Option<SeriesPosition>,
    spine: [Option<DeviceSpineItem>; MAX_DEVICE_SPINE_ITEMS],
    spine_length: u8,
    cover: Option<FixedString<MAX_DEVICE_PATH_BYTES>>,
    navigation: Option<FixedString<MAX_DEVICE_PATH_BYTES>>,
}

impl DevicePublication {
    pub const fn new() -> Self {
        Self {
            title: FixedString::new(),
            creator: FixedString::new(),
            series: FixedString::new(),
            series_position: None,
            spine: [None; MAX_DEVICE_SPINE_ITEMS],
            spine_length: 0,
            cover: None,
            navigation: None,
        }
    }

    #[cfg(any(target_arch = "riscv32", test))]
    pub(crate) unsafe fn initialize_in_place(publication: *mut Self) {
        // SAFETY: the caller provides writable aligned storage; every field is initialized.
        unsafe {
            core::ptr::addr_of_mut!((*publication).title).write(FixedString::new());
            core::ptr::addr_of_mut!((*publication).creator).write(FixedString::new());
            core::ptr::addr_of_mut!((*publication).series).write(FixedString::new());
            core::ptr::addr_of_mut!((*publication).series_position).write(None);
            let spine =
                core::ptr::addr_of_mut!((*publication).spine).cast::<Option<DeviceSpineItem>>();
            for index in 0..MAX_DEVICE_SPINE_ITEMS {
                spine.add(index).write(None);
            }
            core::ptr::addr_of_mut!((*publication).spine_length).write(0);
            core::ptr::addr_of_mut!((*publication).cover).write(None);
            core::ptr::addr_of_mut!((*publication).navigation).write(None);
        }
    }

    fn reset(&mut self) {
        self.title.clear();
        self.creator.clear();
        self.series.clear();
        self.series_position = None;
        self.spine.fill(None);
        self.spine_length = 0;
        self.cover = None;
        self.navigation = None;
    }

    pub fn title(&self) -> &str {
        self.title.as_str()
    }

    pub fn creator(&self) -> &str {
        self.creator.as_str()
    }

    pub fn series(&self) -> &str {
        self.series.as_str()
    }

    pub const fn series_position(&self) -> Option<SeriesPosition> {
        self.series_position
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

impl Default for DevicePublication {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeriesMetadata {
    name: FixedString<MAX_SERIES_NAME_BYTES>,
    position: Option<SeriesPosition>,
}

impl SeriesMetadata {
    pub fn new(name: &str, position: Option<SeriesPosition>) -> Option<Self> {
        let name = FixedString::try_from_str(name).ok()?;
        (!name.is_empty()).then_some(Self { name, position })
    }

    pub fn from_publication(publication: &DevicePublication) -> Option<Self> {
        (!publication.series.is_empty()).then_some(Self {
            name: publication.series,
            position: publication.series_position,
        })
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub const fn position(&self) -> Option<SeriesPosition> {
        self.position
    }

    pub fn as_meta(&self) -> SeriesMeta<'_> {
        SeriesMeta::new(self.name(), self.position)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SeriesScheme {
    Calibre { position_seen: bool },
    Collection { position_seen: bool },
}

pub struct DevicePackageScratch {
    spine_ids: [Option<FixedString<48>>; MAX_DEVICE_SPINE_ITEMS],
    manifest_hashes: [u32; MAX_DEVICE_MANIFEST_ITEMS],
    manifest_length: usize,
    legacy_cover_id: Option<FixedString<48>>,
    series_id: Option<FixedString<48>>,
    pending_refines_id: Option<FixedString<48>>,
    pending_position: Option<SeriesPosition>,
    series_scheme: Option<SeriesScheme>,
}

impl DevicePackageScratch {
    pub const fn new() -> Self {
        Self {
            spine_ids: [None; MAX_DEVICE_SPINE_ITEMS],
            manifest_hashes: [0; MAX_DEVICE_MANIFEST_ITEMS],
            manifest_length: 0,
            legacy_cover_id: None,
            series_id: None,
            pending_refines_id: None,
            pending_position: None,
            series_scheme: None,
        }
    }

    #[cfg(target_arch = "riscv32")]
    pub(crate) unsafe fn initialize_in_place(storage: *mut Self) {
        // SAFETY: each field is initialized in the caller's aligned exclusive storage.
        unsafe {
            let ids =
                core::ptr::addr_of_mut!((*storage).spine_ids).cast::<Option<FixedString<48>>>();
            for index in 0..MAX_DEVICE_SPINE_ITEMS {
                ids.add(index).write(None);
            }
            core::ptr::addr_of_mut!((*storage).manifest_hashes).write_bytes(0, 1);
            core::ptr::addr_of_mut!((*storage).manifest_length).write(0);
            core::ptr::addr_of_mut!((*storage).legacy_cover_id).write(None);
            core::ptr::addr_of_mut!((*storage).series_id).write(None);
            core::ptr::addr_of_mut!((*storage).pending_refines_id).write(None);
            core::ptr::addr_of_mut!((*storage).pending_position).write(None);
            core::ptr::addr_of_mut!((*storage).series_scheme).write(None);
        }
    }

    fn reset(&mut self) {
        self.spine_ids.fill(None);
        self.manifest_length = 0;
        self.legacy_cover_id = None;
        self.series_id = None;
        self.pending_refines_id = None;
        self.pending_position = None;
        self.series_scheme = None;
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

pub struct DeviceEpub<'a, R> {
    archive: StreamingZip<R>,
    publication: &'a DevicePublication,
}

impl<'a, R> DeviceEpub<'a, R>
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
        publication: &'a mut DevicePublication,
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
        parse_package(
            package_path.as_str(),
            &resource_buffer[..package_length],
            package_scratch,
            publication,
        )?;
        Ok(Self {
            archive,
            publication,
        })
    }

    pub const fn publication(&self) -> &DevicePublication {
        self.publication
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

    pub fn read_spine_to(
        &self,
        index: usize,
        inflater: &mut InflateWorkspace,
        buffer: &mut [u8],
        write: impl FnMut(&[u8]) -> Result<(), R::Error>,
    ) -> Result<(), DeviceEpubError<R::Error>> {
        let item = self
            .publication
            .spine_item(index)
            .ok_or(DeviceEpubError::SpineOutOfBounds)?;
        let entry = self
            .archive
            .find(item.path())
            .map_err(DeviceEpubError::Zip)?;
        if entry.uncompressed_size() > MAX_CHAPTER_BYTES {
            return Err(DeviceEpubError::ResourceTooLarge);
        }
        self.archive
            .read_entry_to(entry, inflater, buffer, write)
            .map_err(DeviceEpubError::Zip)
    }

    pub fn read_chapter_titles(
        &self,
        titles: &mut [FixedString<{ crate::navigation::CHAPTER_TITLE_BYTES }>],
        first_spine: usize,
        output: &mut [u8],
        inflater: &mut InflateWorkspace,
    ) -> Result<(), DeviceEpubError<R::Error>> {
        titles.fill(FixedString::new());
        let Some(path) = self.publication.navigation.as_ref() else {
            return Ok(());
        };
        let maximum = output.len().min(64 * 1024);
        let length = self.read_path(path.as_str(), &mut output[..maximum], inflater)?;
        let result = crate::navigation::read_entries(&output[..length], |href, title| {
            let Ok(resolved) = resolve_resource_path::<R::Error>(path.as_str(), href) else {
                return;
            };
            if let Some(index) = self
                .publication
                .spine
                .iter()
                .position(|item| item.as_ref().is_some_and(|item| item.path == resolved))
                && let Some(target) = index
                    .checked_sub(first_spine)
                    .and_then(|index| titles.get_mut(index))
                && target.is_empty()
                && let Ok(title) = FixedString::try_from_str(title)
            {
                *target = title;
            }
        });
        if result.is_err() {
            titles.fill(FixedString::new());
        }
        result.map_err(DeviceEpubError::Xml)
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
    let mut package_path = None;
    while let Some(event) = reader.next_event()? {
        if let XmlEvent::Start(tag) = event
            && package_path.is_none()
            && tag.local_name() == "rootfile"
            && let Some(path) = tag.attribute("full-path")?
        {
            package_path = Some(FixedString::from_decoded(path)?);
        }
    }
    package_path.ok_or(DeviceEpubError::InvalidContainer)
}

#[inline(always)]
fn parse_package<E>(
    package_path: &str,
    encoded: &[u8],
    scratch: &mut DevicePackageScratch,
    publication: &mut DevicePublication,
) -> Result<(), DeviceEpubError<E>> {
    scratch.reset();
    publication.reset();
    parse_package_structure(encoded, scratch, publication)?;
    resolve_manifest(package_path, encoded, scratch, publication)?;
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
    Ok(())
}

fn parse_package_structure<E>(
    encoded: &[u8],
    scratch: &mut DevicePackageScratch,
    publication: &mut DevicePublication,
) -> Result<(), DeviceEpubError<E>> {
    enum MetadataField {
        Title,
        Creator,
        SeriesName,
        GroupPosition,
        CollectionType,
    }

    let mut reader = XmlReader::new(encoded)?;
    let mut in_metadata = false;
    let mut text_field = None;
    let mut series_name = SeriesNameBuffer::new();
    let mut meta_value = FixedString::<32>::new();
    let mut meta_invalid = false;
    let mut meta_declared_id: Option<FixedString<48>> = None;
    let mut meta_refines_id: Option<FixedString<48>> = None;
    while let Some(event) = reader.next_event()? {
        match event {
            XmlEvent::Start(tag) => match tag.local_name() {
                "metadata" => in_metadata = true,
                "title" if in_metadata && publication.title.is_empty() => {
                    text_field = Some(MetadataField::Title)
                }
                "creator" if in_metadata && publication.creator.is_empty() => {
                    text_field = Some(MetadataField::Creator)
                }
                "meta" if in_metadata => {
                    series_name.clear();
                    meta_value.clear();
                    meta_invalid = false;
                    meta_declared_id = None;
                    meta_refines_id = None;
                    let name = tag.attribute("name")?;
                    let content = tag.attribute("content")?;
                    if name == Some("cover")
                        && let Some(id) = content
                    {
                        scratch.legacy_cover_id = Some(FixedString::from_decoded(id)?);
                    }
                    if name == Some("calibre:series") {
                        adopt_calibre_name(publication, scratch, content.unwrap_or(""));
                    }
                    if name == Some("calibre:series_index") {
                        adopt_calibre_position(publication, scratch, content.unwrap_or(""));
                    }
                    match tag.attribute("property")? {
                        Some("belongs-to-collection") => {
                            meta_declared_id = tag
                                .attribute("id")?
                                .and_then(|id| FixedString::from_decoded(id).ok());
                            text_field = Some(MetadataField::SeriesName);
                        }
                        Some("group-position") => {
                            meta_refines_id = refined_id(tag.attribute("refines")?);
                            text_field = Some(MetadataField::GroupPosition);
                        }
                        Some("collection-type") => {
                            meta_refines_id = refined_id(tag.attribute("refines")?);
                            text_field = Some(MetadataField::CollectionType);
                        }
                        _ => {}
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
                Some(MetadataField::Title) => {
                    for character in text {
                        publication.title.push(character?)?;
                    }
                }
                Some(MetadataField::Creator) => {
                    for character in text {
                        publication.creator.push(character?)?;
                    }
                }
                Some(MetadataField::SeriesName) => {
                    for character in text {
                        match character {
                            Ok(character) => series_name.push(character),
                            Err(_) => series_name.invalidate(),
                        }
                    }
                }
                Some(MetadataField::GroupPosition | MetadataField::CollectionType) => {
                    for character in text {
                        match character {
                            Ok(character) => {
                                if meta_value.push(character).is_err() {
                                    meta_invalid = true;
                                }
                            }
                            Err(_) => meta_invalid = true,
                        }
                    }
                }
                None => {}
            },
            XmlEvent::End(name) => match name {
                "metadata" => {
                    in_metadata = false;
                    text_field = None;
                }
                "title" | "creator" => text_field = None,
                "meta" => {
                    match text_field.take() {
                        Some(MetadataField::SeriesName) => adopt_collection_name(
                            publication,
                            scratch,
                            &series_name,
                            meta_declared_id,
                        ),
                        Some(MetadataField::GroupPosition) => adopt_group_position(
                            publication,
                            scratch,
                            meta_refines_id,
                            meta_value.as_str(),
                            meta_invalid,
                        ),
                        Some(MetadataField::CollectionType) => retract_collection(
                            publication,
                            scratch,
                            meta_refines_id,
                            meta_value.as_str(),
                            meta_invalid,
                        ),
                        _ => {}
                    }
                    text_field = None;
                }
                _ => {}
            },
        }
    }
    Ok(())
}

struct SeriesNameBuffer {
    text: FixedString<MAX_SERIES_NAME_BYTES>,
    pending_space: bool,
    invalid: bool,
    full: bool,
}

impl SeriesNameBuffer {
    const fn new() -> Self {
        Self {
            text: FixedString::new(),
            pending_space: false,
            invalid: false,
            full: false,
        }
    }

    fn clear(&mut self) {
        self.text.clear();
        self.pending_space = false;
        self.invalid = false;
        self.full = false;
    }

    fn invalidate(&mut self) {
        self.text.clear();
        self.pending_space = false;
        self.invalid = true;
    }

    fn push(&mut self, character: char) {
        if self.invalid || self.full {
            return;
        }
        if character.is_whitespace() {
            self.pending_space = !self.text.is_empty();
            return;
        }
        let needed = character.len_utf8() + usize::from(self.pending_space);
        if self.text.as_str().len() + needed > MAX_SERIES_NAME_BYTES {
            self.full = true;
            return;
        }
        if self.pending_space {
            self.pending_space = false;
            let _ = self.text.push(' ');
        }
        let _ = self.text.push(character);
    }

    fn fixed(&self) -> FixedString<MAX_SERIES_NAME_BYTES> {
        if self.invalid {
            FixedString::new()
        } else {
            self.text
        }
    }
}

fn refined_id(value: Option<&str>) -> Option<FixedString<48>> {
    FixedString::from_decoded(value?.strip_prefix('#')?).ok()
}

fn adopt_calibre_name(
    publication: &mut DevicePublication,
    scratch: &mut DevicePackageScratch,
    content: &str,
) {
    if scratch.series_scheme.is_some() {
        return;
    }
    let mut name = SeriesNameBuffer::new();
    for character in XmlText::Encoded(content) {
        match character {
            Ok(character) => name.push(character),
            Err(_) => name.invalidate(),
        }
    }
    publication.series = name.fixed();
    publication.series_position = None;
    scratch.series_id = None;
    scratch.series_scheme = Some(SeriesScheme::Calibre {
        position_seen: false,
    });
}

fn adopt_calibre_position(
    publication: &mut DevicePublication,
    scratch: &mut DevicePackageScratch,
    content: &str,
) {
    let Some(SeriesScheme::Calibre { position_seen }) = scratch.series_scheme else {
        return;
    };
    if position_seen {
        return;
    }
    scratch.series_scheme = Some(SeriesScheme::Calibre {
        position_seen: true,
    });
    publication.series_position = SeriesPosition::parse(content);
}

fn adopt_collection_name(
    publication: &mut DevicePublication,
    scratch: &mut DevicePackageScratch,
    name: &SeriesNameBuffer,
    declared_id: Option<FixedString<48>>,
) {
    if scratch.series_scheme.is_some() {
        return;
    }
    publication.series = name.fixed();
    publication.series_position = None;
    scratch.series_id = declared_id;
    let pending = declared_id.is_some() && scratch.pending_refines_id == declared_id;
    if pending {
        publication.series_position = scratch.pending_position;
        scratch.pending_refines_id = None;
        scratch.pending_position = None;
    }
    scratch.series_scheme = Some(SeriesScheme::Collection {
        position_seen: pending,
    });
}

fn adopt_group_position(
    publication: &mut DevicePublication,
    scratch: &mut DevicePackageScratch,
    refines: Option<FixedString<48>>,
    text: &str,
    invalid: bool,
) {
    let Some(refines) = refines else {
        return;
    };
    let position = if invalid {
        None
    } else {
        SeriesPosition::parse(text)
    };
    match scratch.series_scheme {
        Some(SeriesScheme::Collection { position_seen }) => {
            if !position_seen && scratch.series_id == Some(refines) {
                publication.series_position = position;
                scratch.series_scheme = Some(SeriesScheme::Collection {
                    position_seen: true,
                });
            }
        }
        Some(SeriesScheme::Calibre { .. }) => {}
        None => {
            scratch.pending_refines_id = Some(refines);
            scratch.pending_position = position;
        }
    }
}

fn retract_collection(
    publication: &mut DevicePublication,
    scratch: &mut DevicePackageScratch,
    refines: Option<FixedString<48>>,
    value: &str,
    invalid: bool,
) {
    let Some(refines) = refines else {
        return;
    };
    if invalid || value.trim() == "series" || scratch.series_scheme.is_none() {
        return;
    }
    if scratch.series_id == Some(refines) {
        publication.series.clear();
        publication.series_position = None;
        scratch.series_id = None;
        scratch.series_scheme = None;
    }
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
        if properties
            .split_ascii_whitespace()
            .any(|property| property == "nav")
            || (publication.navigation.is_none() && media_type == "application/x-dtbncx+xml")
        {
            publication.navigation = Some(path);
        }
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

pub fn resolve_resource_path<E>(
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

    use std::{boxed::Box, convert::Infallible, format, string::String};

    use super::{DeviceEpub, DevicePackageScratch, DevicePublication, resolve_resource_path};
    use crate::{
        app::SeriesPosition,
        bounded_xml::FixedString,
        zip_stream::{InflateWorkspace, ReadAt, ZipValidationScratch},
    };

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
        let mut publication = Box::new(super::DevicePublication::new());

        let book = DeviceEpub::open(
            SliceFile(encoded),
            &mut zip_scratch,
            &mut package_scratch,
            &mut inflater,
            &mut resource,
            &mut publication,
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
    fn package_metadata_preserves_cdata_and_ignores_empty_title_elements() {
        let mut scratch = DevicePackageScratch::new();
        let xml = br#"<!DOCTYPE package [<!ENTITY custom "unused">]><package><metadata><title/>ignored<title><![CDATA[A &amp; B]]></title><creator><![CDATA[C & D]]></creator><description>&custom;</description></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#;
        let mut publication = DevicePublication::new();
        super::parse_package::<Infallible>("OPS/book.opf", xml, &mut scratch, &mut publication)
            .unwrap();
        assert_eq!(publication.title(), "A &amp; B");
        assert_eq!(publication.creator(), "C & D");
        assert_eq!(
            publication.spine_item(0).unwrap().path(),
            "OPS/chapter.xhtml"
        );
    }

    #[test]
    fn in_place_initialization_writes_the_navigation_field() {
        let mut storage = core::mem::MaybeUninit::<DevicePublication>::uninit();
        unsafe {
            storage.as_mut_ptr().write_bytes(0xa5, 1);
            DevicePublication::initialize_in_place(storage.as_mut_ptr());
        }
        let publication = unsafe { storage.assume_init() };

        assert!(publication.navigation.is_none());
        assert_eq!(publication, DevicePublication::new());
    }

    #[test]
    fn package_reset_drops_navigation_from_the_previous_book() {
        let mut scratch = DevicePackageScratch::new();
        let mut publication = DevicePublication::new();
        let with_navigation = br#"<package><metadata><title>First</title></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine><itemref idref="chapter"/></spine></package>"#;
        let without_navigation = br#"<package><metadata><title>Second</title></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#;

        super::parse_package::<Infallible>(
            "OPS/book.opf",
            with_navigation,
            &mut scratch,
            &mut publication,
        )
        .unwrap();
        assert_eq!(
            publication.navigation.as_ref().map(FixedString::as_str),
            Some("OPS/nav.xhtml")
        );

        super::parse_package::<Infallible>(
            "OPS/book.opf",
            without_navigation,
            &mut scratch,
            &mut publication,
        )
        .unwrap();
        assert!(publication.navigation.is_none());
        assert_eq!(publication.title(), "Second");
    }

    #[test]
    fn container_must_be_complete_before_its_path_is_returned() {
        let result = super::parse_container::<Infallible>(
            br#"<container><rootfile full-path="book.opf"/></wrong>"#,
        );
        assert!(matches!(
            result,
            Err(super::DeviceEpubError::Xml(
                crate::bounded_xml::XmlError::Malformed
            ))
        ));
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

    const SPINE: &str = r#"<manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine>"#;

    fn package(metadata: &str) -> String {
        format!("<package><metadata><title>Book</title>{metadata}</metadata>{SPINE}</package>")
    }

    fn parse(metadata: &str) -> DevicePublication {
        let mut scratch = DevicePackageScratch::new();
        let mut publication = DevicePublication::new();
        super::parse_package::<Infallible>(
            "OPS/book.opf",
            package(metadata).as_bytes(),
            &mut scratch,
            &mut publication,
        )
        .unwrap();
        publication
    }

    #[test]
    fn calibre_series_reads_its_name_and_decimal_index() {
        let publication = parse(
            r#"<meta name="calibre:series" content="Foundation"/><meta name="calibre:series_index" content="1.00"/>"#,
        );
        assert_eq!(publication.series(), "Foundation");
        assert_eq!(publication.series_position(), Some(SeriesPosition::new(1)));
    }

    #[test]
    fn the_first_calibre_declaration_wins_even_when_invalid() {
        let publication = parse(
            r#"<meta name="calibre:series" content="First"/><meta name="calibre:series" content="Second"/><meta name="calibre:series_index" content="1.5"/><meta name="calibre:series_index" content="2"/>"#,
        );
        assert_eq!(publication.series(), "First");
        assert_eq!(publication.series_position(), None);
    }

    #[test]
    fn epub3_collections_accept_refinements_in_either_order() {
        let declaration_first = parse(
            r##"<meta property="belongs-to-collection" id="col">Series X</meta><meta refines="#col" property="collection-type">series</meta><meta refines="#col" property="group-position"> 2 </meta>"##,
        );
        assert_eq!(declaration_first.series(), "Series X");
        assert_eq!(
            declaration_first.series_position(),
            Some(SeriesPosition::new(2))
        );

        let refinement_first = parse(
            r##"<meta refines="#series" property="group-position">3</meta><meta property="belongs-to-collection" id="series">Trilogy</meta>"##,
        );
        assert_eq!(refinement_first.series(), "Trilogy");
        assert_eq!(
            refinement_first.series_position(),
            Some(SeriesPosition::new(3))
        );
    }

    #[test]
    fn collection_type_retraction_lets_a_later_declaration_win() {
        let publication = parse(
            r##"<meta property="belongs-to-collection" id="a">Anthology</meta><meta refines="#a" property="collection-type">anthology</meta><meta name="calibre:series" content="Real Series"/><meta name="calibre:series_index" content="4"/>"##,
        );
        assert_eq!(publication.series(), "Real Series");
        assert_eq!(publication.series_position(), Some(SeriesPosition::new(4)));
    }

    #[test]
    fn refinements_for_another_collection_never_attach() {
        let publication = parse(
            r##"<meta refines="#a" property="group-position">5</meta><meta property="belongs-to-collection" id="b">Other</meta>"##,
        );
        assert_eq!(publication.series(), "Other");
        assert_eq!(publication.series_position(), None);
    }

    #[test]
    fn collection_names_normalize_whitespace_before_storage() {
        let publication = parse(
            "<meta property=\"belongs-to-collection\" id=\"c\">  The   Long\n  Series </meta><meta refines=\"#c\" property=\"group-position\"> 2 </meta>",
        );
        assert_eq!(publication.series(), "The Long Series");
        assert_eq!(publication.series_position(), Some(SeriesPosition::new(2)));
    }

    #[test]
    fn oversized_series_names_truncate_instead_of_dropping_the_book() {
        let name = "Å".repeat(200);
        let publication = parse(&format!(
            "<meta name=\"calibre:series\" content=\"{name}\"/>"
        ));
        assert_eq!(publication.series(), "Å".repeat(64));
        assert_eq!(publication.title(), "Book");

        let mixed = parse(&format!(
            "<meta name=\"calibre:series\" content=\"{}ÅZ\"/>",
            "A".repeat(127)
        ));
        assert_eq!(mixed.series(), "A".repeat(127));
    }

    #[test]
    fn undecodable_series_text_yields_no_series_or_position() {
        let name = parse("<meta property=\"belongs-to-collection\" id=\"c\">Bad&bogus;Name</meta>");
        assert_eq!(name.series(), "");
        assert_eq!(name.series_position(), None);

        let position = parse(
            "<meta property=\"belongs-to-collection\" id=\"c\">Series</meta><meta refines=\"#c\" property=\"group-position\">1&bogus;2</meta>",
        );
        assert_eq!(position.series(), "Series");
        assert_eq!(position.series_position(), None);
    }

    #[test]
    fn oversized_and_undecodable_ids_only_skip_correlation() {
        let id = "i".repeat(120);
        let oversized = parse(&format!(
            "<meta property=\"belongs-to-collection\" id=\"{id}\">Series</meta><meta refines=\"#{id}\" property=\"group-position\">7</meta>"
        ));
        assert_eq!(oversized.series(), "Series");
        assert_eq!(oversized.series_position(), None);

        let undecodable = parse(
            r##"<meta property="belongs-to-collection" id="&bogus;">Named</meta><meta refines="#&bogus;" property="group-position">5</meta>"##,
        );
        assert_eq!(undecodable.series(), "Named");
        assert_eq!(undecodable.series_position(), None);
    }

    #[test]
    fn oversized_group_position_text_degrades_to_no_position() {
        let text = format!("2{}3", " ".repeat(64));
        let publication = parse(&format!(
            "<meta property=\"belongs-to-collection\" id=\"c\">Series</meta><meta refines=\"#c\" property=\"group-position\">{text}</meta>"
        ));
        assert_eq!(publication.series(), "Series");
        assert_eq!(publication.series_position(), None);
    }

    #[test]
    fn series_state_resets_between_books_in_the_same_scratch() {
        let mut scratch = DevicePackageScratch::new();
        let mut publication = DevicePublication::new();
        let with_series = package(
            r#"<meta name="calibre:series" content="First Series"/><meta name="calibre:series_index" content="2"/>"#,
        );
        let without_series = package("");

        super::parse_package::<Infallible>(
            "OPS/book.opf",
            with_series.as_bytes(),
            &mut scratch,
            &mut publication,
        )
        .unwrap();
        assert_eq!(publication.series(), "First Series");
        assert_eq!(publication.series_position(), Some(SeriesPosition::new(2)));

        super::parse_package::<Infallible>(
            "OPS/book.opf",
            without_series.as_bytes(),
            &mut scratch,
            &mut publication,
        )
        .unwrap();
        assert_eq!(publication.series(), "");
        assert_eq!(publication.series_position(), None);
    }

    #[test]
    fn series_metadata_outside_the_metadata_block_is_ignored() {
        let xml = format!(
            "<package><metadata><title>Book</title></metadata><meta name=\"calibre:series\" content=\"Outside\"/>{SPINE}</package>"
        );
        let mut scratch = DevicePackageScratch::new();
        let mut publication = DevicePublication::new();
        super::parse_package::<Infallible>(
            "OPS/book.opf",
            xml.as_bytes(),
            &mut scratch,
            &mut publication,
        )
        .unwrap();
        assert_eq!(publication.series(), "");
    }
}
