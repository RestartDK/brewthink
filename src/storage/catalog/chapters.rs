use super::{images::FileSource, *};
use crate::{
    bounded_layout::{BoundedPage, StreamLayoutError, layout_xhtml_stream},
    chapter_cache::{
        CHAPTER_HEADER_BYTES, ChapterHeader, ChapterKey, ChapterRequest, ChapterSourceKey,
        ChapterText, ChapterWorkspace, LayoutWorkspace, MAX_CHAPTER_CACHE_BYTES, MAX_CHAPTER_PAGES,
        PreparedChapter, SOURCE_HEADER_BYTES,
    },
    device_epub::resolve_resource_path,
    image_cache::{CacheState, ImageProbeError, ImageWorkspace},
    zip_stream::{StreamingZip, ZipError, ZipValidationScratch},
};
use core::{cell::RefCell, fmt::Write};

const CACHE_BYTES: u64 = 64 * 1024 * 1024;
const CACHE_ENTRIES: usize = 128;

impl<D: BlockDevice, T: TimeSource, const DIRS: usize, const FILES: usize, const VOLUMES: usize>
    AppDataStore<'_, D, T, DIRS, FILES, VOLUMES>
{
    pub fn chapter_page(
        &self,
        request: ChapterRequest<'_>,
        workspace: &mut ChapterWorkspace,
        images: &mut ImageWorkspace,
        zip: &mut ZipValidationScratch,
        page: &mut BoundedPage,
    ) -> Result<PreparedChapter, AppDataError<D::Error>> {
        let volume = self
            .storage
            .manager
            .open_volume(VolumeIdx(0))
            .map_err(AppDataError::Filesystem)?;
        let root = volume.open_root_dir().map_err(AppDataError::Filesystem)?;
        let app = root
            .open_dir(APP_DATA_DIRECTORY)
            .map_err(AppDataError::Filesystem)?;
        let cache = app
            .open_dir(CACHE_DIRECTORY)
            .map_err(AppDataError::Filesystem)?;
        app.close().map_err(AppDataError::Filesystem)?;
        let books = root
            .open_dir(BOOK_DIRECTORY)
            .map_err(AppDataError::Filesystem)?;
        root.close().map_err(AppDataError::Filesystem)?;
        let file = FatStorage::open_book_file(&books, request.book.name())
            .map_err(AppDataError::Filesystem)?;
        let archive = StreamingZip::open(FileSource::new(&file), zip).map_err(chapter_zip_error)?;
        let entry = archive.find(request.path).map_err(chapter_zip_error)?;
        let key = ChapterKey::new(
            ChapterSourceKey::new(
                request.book,
                entry,
                archive.directory_crc32().map_err(chapter_zip_error)?,
            )
            .ok_or(AppDataError::FileTooLarge)?,
            request.preferences,
        );
        let workspace = workspace.layout();
        if let Some(header) =
            Self::read_chapter_page(&cache, &key, request.page_index, workspace, page)?
        {
            return Ok(PreparedChapter {
                key,
                state: CacheState::Hit,
                summary: header.summary,
            });
        }
        let source_name = key.source().file_name();
        if !Self::valid_chapter_text(&cache, key.source(), &mut workspace.buffer)? {
            Self::reserve_chapter_cache(
                &cache,
                source_name.as_str(),
                SOURCE_HEADER_BYTES as u64 + u64::from(key.source().length()),
                None,
            )?;
            let staged = cache
                .open_file_in_dir(source_name.as_str(), Mode::ReadWriteCreateOrTruncate)
                .map_err(AppDataError::Filesystem)?;
            staged
                .write(&[0; SOURCE_HEADER_BYTES])
                .map_err(AppDataError::Filesystem)?;
            archive
                .read_entry_to(entry, images.inflate(), &mut workspace.buffer, |bytes| {
                    staged.write(bytes)
                })
                .map_err(chapter_zip_error)?;
            staged.flush().map_err(AppDataError::Filesystem)?;
            staged
                .seek_from_start(0)
                .map_err(AppDataError::Filesystem)?;
            staged
                .write(&key.source().header())
                .map_err(AppDataError::Filesystem)?;
            staged.close().map_err(AppDataError::Filesystem)?;
            if !Self::valid_chapter_text(&cache, key.source(), &mut workspace.buffer)? {
                return Err(AppDataError::ChecksumMismatch);
            }
        }
        file.close().map_err(AppDataError::Filesystem)?;
        Self::build_chapter(&cache, &books, &key, workspace, images, zip, page)?;
        let header = Self::read_chapter_page(&cache, &key, request.page_index, workspace, page)?
            .ok_or(AppDataError::ChecksumMismatch)?;
        books
            .close()
            .and(cache.close())
            .and(volume.close())
            .map_err(AppDataError::Filesystem)?;
        Ok(PreparedChapter {
            key,
            state: CacheState::Prepared,
            summary: header.summary,
        })
    }

    fn read_chapter_exact(
        file: &File<'_, D, T, DIRS, FILES, VOLUMES>,
        mut bytes: &mut [u8],
    ) -> Result<(), Error<D::Error>> {
        while !bytes.is_empty() {
            let count = file.read(bytes)?;
            if count == 0 {
                return Err(Error::EndOfFile);
            }
            bytes = &mut bytes[count..];
        }
        Ok(())
    }

    fn valid_chapter_text(
        cache: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        key: &ChapterSourceKey,
        buffer: &mut [u8],
    ) -> Result<bool, AppDataError<D::Error>> {
        let file = match cache.open_file_in_dir(key.file_name().as_str(), Mode::ReadOnly) {
            Ok(file) => file,
            Err(Error::NotFound) => return Ok(false),
            Err(error) => return Err(AppDataError::Filesystem(error)),
        };
        let valid = (|| {
            if file.length() != SOURCE_HEADER_BYTES as u32 + key.length() {
                return Ok(false);
            }
            let mut header = [0; SOURCE_HEADER_BYTES];
            Self::read_chapter_exact(&file, &mut header)?;
            if !key.matches_header(&header, file.length()) {
                return Ok(false);
            }
            let mut checksum = Hasher::new();
            let mut remaining = key.length() as usize;
            while remaining > 0 {
                let count = remaining.min(buffer.len());
                Self::read_chapter_exact(&file, &mut buffer[..count])?;
                checksum.update(&buffer[..count]);
                remaining -= count;
            }
            Ok(checksum.finalize() == key.checksum())
        })();
        file.close().map_err(AppDataError::Filesystem)?;
        valid.map_err(AppDataError::Filesystem)
    }

    fn read_chapter_page(
        cache: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        key: &ChapterKey,
        page_index: usize,
        workspace: &mut LayoutWorkspace,
        page: &mut BoundedPage,
    ) -> Result<Option<ChapterHeader>, AppDataError<D::Error>> {
        let file = match cache.open_file_in_dir(key.file_name().as_str(), Mode::ReadOnly) {
            Ok(file) => file,
            Err(Error::NotFound) => return Ok(None),
            Err(error) => return Err(AppDataError::Filesystem(error)),
        };
        let result = (|| {
            if file.length() < CHAPTER_HEADER_BYTES as u32 {
                return Ok(None);
            }
            let mut bytes = [0; CHAPTER_HEADER_BYTES];
            Self::read_chapter_exact(&file, &mut bytes).map_err(AppDataError::Filesystem)?;
            let Some(header) = ChapterHeader::decode(&bytes, key, file.length()) else {
                return Ok(None);
            };
            if page_index >= header.summary.page_count {
                return Err(AppDataError::InvalidMetadata);
            }
            let index = &mut workspace.index[..(header.summary.page_count + 1) * 4];
            file.seek_from_start(header.index_offset)
                .map_err(AppDataError::Filesystem)?;
            Self::read_chapter_exact(&file, index).map_err(AppDataError::Filesystem)?;
            let Some(range) = header.page_range(index, page_index) else {
                return Ok(None);
            };
            file.seek_from_start(range.start)
                .map_err(AppDataError::Filesystem)?;
            let mut record_header = [0; 8];
            Self::read_chapter_exact(&file, &mut record_header)
                .map_err(AppDataError::Filesystem)?;
            let length = u32::from_le_bytes(record_header[..4].try_into().unwrap());
            if length != range.end - range.start - 8 {
                return Ok(None);
            }
            let Some(record) = workspace.record.get_mut(..length as usize) else {
                return Ok(None);
            };
            Self::read_chapter_exact(&file, record).map_err(AppDataError::Filesystem)?;
            if crc32fast::hash(record) != u32::from_le_bytes(record_header[4..].try_into().unwrap())
                || page
                    .decode(record, page_index, header.summary, key.preferences())
                    .is_none()
            {
                return Ok(None);
            }
            Ok(Some(header))
        })();
        file.close().map_err(AppDataError::Filesystem)?;
        result
    }

    fn build_chapter(
        cache: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        books: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        key: &ChapterKey,
        workspace: &mut LayoutWorkspace,
        images: &mut ImageWorkspace,
        zip: &mut ZipValidationScratch,
        page: &mut BoundedPage,
    ) -> Result<(), AppDataError<D::Error>> {
        let source_name = key.source().file_name();
        let name = key.file_name();
        Self::reserve_chapter_cache(
            cache,
            name.as_str(),
            CHAPTER_HEADER_BYTES as u64,
            Some(source_name.as_str()),
        )?;
        let source = cache
            .open_file_in_dir(source_name.as_str(), Mode::ReadOnly)
            .map_err(AppDataError::Filesystem)?;
        let reader = FileSource::new(&source);
        let text = ChapterText::new(&reader, key.source()).ok_or(AppDataError::InvalidMetadata)?;
        let file = cache
            .open_file_in_dir(name.as_str(), Mode::ReadWriteCreateOrTruncate)
            .map_err(AppDataError::Filesystem)?;
        file.write(&[0; CHAPTER_HEADER_BYTES])
            .map_err(AppDataError::Filesystem)?;
        let writer = RefCell::new(Some(file));
        let mut count = 0usize;
        let mut position = CHAPTER_HEADER_BYTES as u32;
        let summary = layout_xhtml_stream(
            &text,
            &mut workspace.xml,
            key.preferences(),
            page,
            |href| {
                if let Some(file) = writer.borrow_mut().take() {
                    file.close()?;
                }
                let Ok(path) = resolve_resource_path::<()>(key.source().path(), href) else {
                    return Ok(None);
                };
                let file = FatStorage::open_book_file(books, key.source().book().name())?;
                let probe = (|| {
                    let archive = StreamingZip::open(FileSource::new(&file), zip)?;
                    match images.probe_resource(&archive, path.as_str()) {
                        Ok(resource) => Ok(Some(resource)),
                        Err(ImageProbeError::Image(_)) => Ok(None),
                        Err(ImageProbeError::Read(error)) => Err(ZipError::Read(error)),
                    }
                })();
                file.close()?;
                match probe {
                    Ok(resource) => Ok(resource),
                    Err(ZipError::Read(error) | ZipError::Write(error)) => Err(error),
                    Err(_) => Err(Error::FormatError(
                        "invalid image archive during chapter layout",
                    )),
                }
            },
            |page| {
                if count == MAX_CHAPTER_PAGES || page.page_index() != count {
                    return Err(Error::FormatError("chapter page limit exceeded"));
                }
                let length = page
                    .encode(&mut workspace.record)
                    .ok_or(Error::FormatError("invalid page record"))?;
                let next = position
                    .checked_add(length as u32 + 8)
                    .ok_or(Error::NotEnoughSpace)?;
                let expected = u64::from(next) + (count as u64 + 2) * 4;
                if expected > u64::from(MAX_CHAPTER_CACHE_BYTES) {
                    return Err(Error::NotEnoughSpace);
                }
                Self::reserve_chapter_cache(
                    cache,
                    name.as_str(),
                    expected,
                    Some(source_name.as_str()),
                )
                .map_err(|error| match error {
                    AppDataError::Filesystem(error) => error,
                    _ => Error::NotEnoughSpace,
                })?;
                let mut writer = writer.borrow_mut();
                if writer.is_none() {
                    *writer = Some(cache.open_file_in_dir(name.as_str(), Mode::ReadWriteAppend)?);
                }
                let file = writer.as_ref().ok_or(Error::BadHandle)?;
                let bytes = &workspace.record[..length];
                file.write(&(length as u32).to_le_bytes())?;
                file.write(&crc32fast::hash(bytes).to_le_bytes())?;
                file.write(bytes)?;
                workspace.index[count * 4..count * 4 + 4].copy_from_slice(&position.to_le_bytes());
                position = next;
                count += 1;
                Ok(())
            },
        )
        .map_err(|error| match error {
            StreamLayoutError::Read(error) | StreamLayoutError::Write(error) => {
                AppDataError::Filesystem(error)
            }
            StreamLayoutError::Layout(error) => AppDataError::Chapter(error),
        })?;
        if text.checksum() != Some(key.source().checksum()) {
            return Err(AppDataError::ChecksumMismatch);
        }
        source.close().map_err(AppDataError::Filesystem)?;
        workspace.index[count * 4..count * 4 + 4].copy_from_slice(&position.to_le_bytes());
        let index = &workspace.index[..(count + 1) * 4];
        let header = ChapterHeader {
            summary,
            index_offset: position,
            file_length: position + index.len() as u32,
            index_crc: crc32fast::hash(index),
        };
        let file = writer.into_inner().ok_or(AppDataError::IncompleteWrite)?;
        file.write(index).map_err(AppDataError::Filesystem)?;
        file.flush().map_err(AppDataError::Filesystem)?;
        file.seek_from_start(0).map_err(AppDataError::Filesystem)?;
        file.write(&header.encode(key))
            .map_err(AppDataError::Filesystem)?;
        file.close().map_err(AppDataError::Filesystem)
    }

    fn reserve_chapter_cache(
        cache: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        name: &str,
        bytes: u64,
        protected: Option<&str>,
    ) -> Result<(), AppDataError<D::Error>> {
        loop {
            let mut total = bytes;
            let mut count = 1usize;
            let mut victim = None;
            cache
                .iterate_dir(|entry| {
                    if entry.attributes.is_directory()
                        || !matches!(entry.name.extension(), b"HTM" | b"PGS")
                        || entry.name.base_name().len() != 8
                        || !entry.name.base_name().iter().all(u8::is_ascii_hexdigit)
                    {
                        return ControlFlow::Continue(());
                    }
                    let mut candidate = ShortName::new();
                    if write!(candidate, "{}", entry.name).is_err() || candidate.as_str() == name {
                        return ControlFlow::Continue(());
                    }
                    total += u64::from(entry.size);
                    count += 1;
                    if protected != Some(candidate.as_str()) && victim.is_none() {
                        victim = Some(entry.name);
                    }
                    ControlFlow::Continue(())
                })
                .map_err(AppDataError::Filesystem)?;
            if total <= CACHE_BYTES && count <= CACHE_ENTRIES {
                return Ok(());
            }
            let victim = victim.ok_or(AppDataError::FileTooLarge)?;
            cache
                .delete_entry_in_dir(victim)
                .map_err(AppDataError::Filesystem)?;
        }
    }
}

fn chapter_zip_error<E: core::error::Error>(error: ZipError<Error<E>>) -> AppDataError<E> {
    match error {
        ZipError::Read(error) | ZipError::Write(error) => AppDataError::Filesystem(error),
        _ => AppDataError::InvalidMetadata,
    }
}
