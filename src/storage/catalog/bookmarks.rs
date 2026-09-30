use super::*;
use crate::{app::BookProgress, bounded_xml::FixedString, storage::book_resume::BookIdentity};
use core::fmt::Write;
use embedded_sdmmc::{FilenameError, ShortFileName, filesystem::ToShortFileName};

const BOOKMARK_MAGIC: u32 = 0x4254_4231;
pub(super) const BOOKMARK_BYTES: usize = 4 + 4 + MAX_BOOK_NAME_BYTES + 6 * 4;
const CHECKSUM_OFFSET: usize = BOOKMARK_BYTES - 4;
pub(super) const BOOKMARK_PROBES: u32 = 32;
pub(super) type BookmarkBytes = [u8; BOOKMARK_BYTES];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BookmarkSlot(u32);

#[derive(Clone, Copy)]
pub(super) enum BookmarkFile {
    Primary,
    Backup,
    Temporary,
}

impl From<BookIdentity> for BookmarkSlot {
    fn from(identity: BookIdentity) -> Self {
        let mut hasher = Hasher::new();
        hasher.update(identity.file_name().as_str().as_bytes());
        hasher.update(&identity.size().to_le_bytes());
        Self(hasher.finalize())
    }
}

impl BookmarkSlot {
    pub(super) fn probe(self, offset: u32) -> Self {
        Self(self.0.wrapping_add(offset))
    }

    pub(super) fn name(self, kind: BookmarkFile) -> Result<ShortFileName, FilenameError> {
        let extension = match kind {
            BookmarkFile::Primary => "BMK",
            BookmarkFile::Backup => "BAK",
            BookmarkFile::Temporary => "TMP",
        };
        let mut name = FixedString::<12>::new();
        let hash = self.0;
        write!(name, "{hash:08X}.{extension}").map_err(|_| FilenameError::NameTooLong)?;
        name.as_str().to_short_filename()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookmarkDecodeError {
    RecordLength,
    Magic,
    Checksum,
    IdentityLength,
    IdentityInvalid,
    ProgressInvalid,
}

impl fmt::Display for BookmarkDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::RecordLength => "bookmark record length is invalid",
            Self::Magic => "bookmark magic mismatch",
            Self::Checksum => "bookmark checksum mismatch",
            Self::IdentityLength => "bookmark identity length is invalid",
            Self::IdentityInvalid => "bookmark filename is invalid",
            Self::ProgressInvalid => "bookmark progress is invalid",
        })
    }
}
impl core::error::Error for BookmarkDecodeError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BookmarkRecord {
    pub(super) identity: BookIdentity,
    pub(super) progress: BookProgress,
}

impl From<BookmarkRecord> for BookmarkBytes {
    fn from(record: BookmarkRecord) -> Self {
        let mut bytes = [0; BOOKMARK_BYTES];
        let name = record.identity.file_name();
        let name = name.as_str().as_bytes();
        bytes[0..4].copy_from_slice(&BOOKMARK_MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&(name.len() as u32).to_le_bytes());
        bytes[8..8 + name.len()].copy_from_slice(name);
        let offset = 8 + MAX_BOOK_NAME_BYTES;
        let progress = record.progress;
        for (index, value) in [
            record.identity.size(),
            progress.spine_index() as u32,
            progress.page_index() as u32,
            progress.page_count() as u32,
            progress.preferences().packed(),
        ]
        .into_iter()
        .enumerate()
        {
            bytes[offset + index * 4..offset + (index + 1) * 4]
                .copy_from_slice(&value.to_le_bytes());
        }
        let checksum = crc32fast::hash(&bytes[..CHECKSUM_OFFSET]);
        bytes[CHECKSUM_OFFSET..].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }
}

impl TryFrom<&BookmarkBytes> for BookmarkRecord {
    type Error = BookmarkDecodeError;
    fn try_from(bytes: &BookmarkBytes) -> Result<Self, Self::Error> {
        let word = |offset| {
            u32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        if word(0) != BOOKMARK_MAGIC {
            return Err(BookmarkDecodeError::Magic);
        }
        if crc32fast::hash(&bytes[..CHECKSUM_OFFSET]) != word(CHECKSUM_OFFSET) {
            return Err(BookmarkDecodeError::Checksum);
        }
        let length = word(4) as usize;
        if length == 0 || length > MAX_BOOK_NAME_BYTES {
            return Err(BookmarkDecodeError::IdentityLength);
        }
        let name = core::str::from_utf8(&bytes[8..8 + length])
            .map_err(|_| BookmarkDecodeError::IdentityInvalid)?;
        let name =
            BookFileName::try_from(name).map_err(|_| BookmarkDecodeError::IdentityInvalid)?;
        let offset = 8 + MAX_BOOK_NAME_BYTES;
        let identity = BookIdentity::new(name, word(offset));
        let preferences = crate::app::ReaderPreferences::from_packed(word(offset + 16))
            .ok_or(BookmarkDecodeError::ProgressInvalid)?;
        let progress = BookProgress::new(
            word(offset + 4) as usize,
            word(offset + 8) as usize,
            word(offset + 12) as usize,
            preferences,
        )
        .ok_or(BookmarkDecodeError::ProgressInvalid)?;
        Ok(Self { identity, progress })
    }
}

struct ResolvedBookmark {
    slot: BookmarkSlot,
    progress: Result<Option<BookProgress>, BookmarkDecodeError>,
}

impl<D, T, const DIRS: usize, const FILES: usize, const VOLUMES: usize>
    AppDataStore<'_, D, T, DIRS, FILES, VOLUMES>
where
    D: BlockDevice,
    T: TimeSource,
{
    pub fn read_book_progress(
        &self,
        book: &BookFile,
    ) -> Result<Option<BookProgress>, AppDataError<D::Error>> {
        self.storage.with_app_directory(|app| {
            let directory = match app.open_dir(BOOKMARK_DIRECTORY) {
                Ok(directory) => directory,
                Err(Error::NotFound) => return Ok(Ok(None)),
                Err(error) => return Err(error),
            };
            let result = Self::resolve_bookmark(&directory, BookIdentity::from(book))
                .and_then(|resolved| resolved.progress.map_err(AppDataError::Bookmark));
            directory.close()?;
            Ok(result)
        })?
    }

    pub fn write_book_progress(
        &self,
        book: &BookFile,
        progress: BookProgress,
    ) -> Result<(), AppDataError<D::Error>> {
        let identity = BookIdentity::from(book);
        let bytes = BookmarkBytes::from(BookmarkRecord { identity, progress });
        self.storage
            .ensure_layout()
            .map_err(AppDataError::Filesystem)?;
        self.storage.with_app_directory(|app| {
            let directory = app.open_dir(BOOKMARK_DIRECTORY)?;
            let result = Self::resolve_bookmark(&directory, identity).and_then(|resolved| {
                Self::publish_bookmark(&directory, resolved.slot, identity, &bytes)
            });
            directory.close()?;
            Ok(result)
        })?
    }

    fn resolve_bookmark(
        directory: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        identity: BookIdentity,
    ) -> Result<ResolvedBookmark, AppDataError<D::Error>> {
        let base = BookmarkSlot::from(identity);
        let mut vacant = None;
        for offset in 0..BOOKMARK_PROBES {
            let slot = base.probe(offset);
            let mut occupied = false;
            let mut matching = false;
            let mut progress = Ok(None);
            for kind in [
                BookmarkFile::Primary,
                BookmarkFile::Backup,
                BookmarkFile::Temporary,
            ] {
                let name = slot
                    .name(kind)
                    .map_err(|error| AppDataError::Filesystem(Error::FilenameError(error)))?;
                let decoded = match Self::read_bookmark_file(directory, name) {
                    Ok(Some(bytes)) => BookmarkRecord::try_from(&bytes),
                    Ok(None) => continue,
                    Err(AppDataError::Bookmark(error)) => Err(error),
                    Err(error) => return Err(error),
                };
                match decoded {
                    Ok(record) if record.identity != identity => occupied = true,
                    Ok(record) => {
                        matching = true;
                        if !matches!(kind, BookmarkFile::Temporary)
                            && !matches!(progress, Ok(Some(_)))
                        {
                            progress = Ok(Some(record.progress));
                        }
                    }
                    Err(error) => {
                        if !matches!(kind, BookmarkFile::Temporary)
                            && !matches!(progress, Ok(Some(_)))
                        {
                            progress = Err(error);
                        }
                    }
                }
            }
            if !occupied {
                let resolved = ResolvedBookmark { slot, progress };
                if matching {
                    return Ok(resolved);
                }
                if vacant.is_none() {
                    vacant = Some(resolved);
                }
            }
        }
        vacant.ok_or(AppDataError::BookmarkSlotsExhausted)
    }

    fn publish_bookmark(
        directory: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        slot: BookmarkSlot,
        identity: BookIdentity,
        bytes: &BookmarkBytes,
    ) -> Result<(), AppDataError<D::Error>> {
        let primary = slot
            .name(BookmarkFile::Primary)
            .map_err(|error| AppDataError::Filesystem(Error::FilenameError(error)))?;
        let backup = slot
            .name(BookmarkFile::Backup)
            .map_err(|error| AppDataError::Filesystem(Error::FilenameError(error)))?;
        let temporary = slot
            .name(BookmarkFile::Temporary)
            .map_err(|error| AppDataError::Filesystem(Error::FilenameError(error)))?;
        Self::write_bookmark_file(directory, temporary, bytes)?;
        if Self::read_bookmark_file(directory, temporary)?.as_ref() != Some(bytes) {
            return Err(AppDataError::IncompleteWrite);
        }
        let existing = match Self::read_bookmark_file(directory, primary) {
            Ok(existing) => {
                existing.map(|bytes| BookmarkRecord::try_from(&bytes).map(|record| (bytes, record)))
            }
            Err(AppDataError::Bookmark(error)) => Some(Err(error)),
            Err(error) => return Err(error),
        };
        if let Some(existing) = existing {
            match existing {
                Ok((bytes, record)) if record.identity == identity => {
                    Self::copy_bookmark(directory, primary, backup)?;
                    if Self::read_bookmark_file(directory, backup)? != Some(bytes) {
                        return Err(AppDataError::IncompleteWrite);
                    }
                }
                Ok(_) => return Err(AppDataError::TargetExists),
                Err(
                    BookmarkDecodeError::RecordLength
                    | BookmarkDecodeError::Magic
                    | BookmarkDecodeError::Checksum
                    | BookmarkDecodeError::IdentityLength
                    | BookmarkDecodeError::IdentityInvalid
                    | BookmarkDecodeError::ProgressInvalid,
                ) => {}
            }
        }
        match directory.delete_entry_in_dir(primary) {
            Ok(()) | Err(Error::NotFound) => {}
            Err(error) => return Err(AppDataError::Filesystem(error)),
        }
        Self::copy_bookmark(directory, temporary, primary)?;
        if Self::read_bookmark_file(directory, primary)?.as_ref() != Some(bytes) {
            return Err(AppDataError::IncompleteWrite);
        }
        directory
            .delete_entry_in_dir(temporary)
            .map_err(AppDataError::Filesystem)
    }

    fn write_bookmark_file(
        directory: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        name: embedded_sdmmc::ShortFileName,
        bytes: &[u8; BOOKMARK_BYTES],
    ) -> Result<(), AppDataError<D::Error>> {
        let file = directory
            .open_file_in_dir(name, Mode::ReadWriteCreateOrTruncate)
            .map_err(AppDataError::Filesystem)?;
        file.write(bytes).map_err(AppDataError::Filesystem)?;
        file.close().map_err(AppDataError::Filesystem)
    }

    fn copy_bookmark(
        directory: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        source: embedded_sdmmc::ShortFileName,
        target: embedded_sdmmc::ShortFileName,
    ) -> Result<(), AppDataError<D::Error>> {
        let source = directory
            .open_file_in_dir(source, Mode::ReadOnly)
            .map_err(AppDataError::Filesystem)?;
        let target = directory
            .open_file_in_dir(target, Mode::ReadWriteCreateOrTruncate)
            .map_err(AppDataError::Filesystem)?;
        let mut copied = [0; BOOKMARK_BYTES];
        let mut offset = 0;
        while offset < BOOKMARK_BYTES {
            let count = source
                .read(&mut copied[offset..])
                .map_err(AppDataError::Filesystem)?;
            if count == 0 {
                break;
            }
            target
                .write(&copied[offset..offset + count])
                .map_err(AppDataError::Filesystem)?;
            offset += count;
        }
        target.close().map_err(AppDataError::Filesystem)?;
        source.close().map_err(AppDataError::Filesystem)
    }

    fn read_bookmark_file(
        directory: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        name: embedded_sdmmc::ShortFileName,
    ) -> Result<Option<[u8; BOOKMARK_BYTES]>, AppDataError<D::Error>> {
        let file = match directory.open_file_in_dir(name, Mode::ReadOnly) {
            Ok(file) => file,
            Err(Error::NotFound) => return Ok(None),
            Err(error) => return Err(AppDataError::Filesystem(error)),
        };
        if file.length() != BOOKMARK_BYTES as u32 {
            file.close().map_err(AppDataError::Filesystem)?;
            return Err(AppDataError::Bookmark(BookmarkDecodeError::RecordLength));
        }
        let mut bytes = [0; BOOKMARK_BYTES];
        let mut offset = 0;
        while offset < BOOKMARK_BYTES {
            let count = file
                .read(&mut bytes[offset..])
                .map_err(AppDataError::Filesystem)?;
            if count == 0 {
                break;
            }
            offset += count;
        }
        file.close().map_err(AppDataError::Filesystem)?;
        if offset != BOOKMARK_BYTES {
            return Err(AppDataError::Bookmark(BookmarkDecodeError::RecordLength));
        }
        Ok(Some(bytes))
    }
}
