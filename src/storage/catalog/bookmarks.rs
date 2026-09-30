use super::*;
use crate::storage::book_resume::{BookIdentity, BookProgress};

const BOOKMARK_MAGIC: u32 = 0x4254_4231;
const BOOKMARK_BYTES: usize = 4 + 4 + MAX_BOOK_NAME_BYTES + 6 * 4;
const CHECKSUM_OFFSET: usize = BOOKMARK_BYTES - 4;
const STORED_EXTENSION: [u8; 3] = *b"BMK";
const BACKUP_EXTENSION: [u8; 3] = *b"BAK";
const TEMP_EXTENSION: [u8; 3] = *b"TMP";
const HEX: [u8; 16] = *b"0123456789ABCDEF";

pub(super) struct BookmarkNames {
    pub(super) stored: [u8; 12],
    pub(super) backup: [u8; 12],
    pub(super) temp: [u8; 12],
}

impl BookmarkNames {
    pub(super) fn new(identity: BookIdentity) -> Self {
        let mut hasher = Hasher::new();
        hasher.update(identity.file_name().as_str().as_bytes());
        hasher.update(&identity.size().to_le_bytes());
        let hash = hasher.finalize().to_be_bytes();
        let mut stored = [0; 12];
        for (index, byte) in hash.iter().enumerate() {
            stored[index * 2] = HEX[usize::from(byte >> 4)];
            stored[index * 2 + 1] = HEX[usize::from(byte & 0x0F)];
        }
        stored[8] = b'.';
        stored[9..12].copy_from_slice(&STORED_EXTENSION);
        let mut backup = stored;
        backup[9..12].copy_from_slice(&BACKUP_EXTENSION);
        let mut temp = stored;
        temp[9..12].copy_from_slice(&TEMP_EXTENSION);
        Self {
            stored,
            backup,
            temp,
        }
    }

    fn backup(&self) -> &str {
        core::str::from_utf8(&self.backup).expect("synthetic bookmark name is ASCII")
    }

    fn stored(&self) -> &str {
        core::str::from_utf8(&self.stored).expect("synthetic bookmark name is ASCII")
    }

    fn temp(&self) -> &str {
        core::str::from_utf8(&self.temp).expect("synthetic bookmark name is ASCII")
    }
}

fn encode_bookmark(book: &BookFile, progress: BookProgress) -> [u8; BOOKMARK_BYTES] {
    let mut bytes = [0; BOOKMARK_BYTES];
    let name = book.name().as_str().as_bytes();
    bytes[0..4].copy_from_slice(&BOOKMARK_MAGIC.to_le_bytes());
    bytes[4..8].copy_from_slice(&(name.len() as u32).to_le_bytes());
    bytes[8..8 + name.len()].copy_from_slice(name);
    let offset = 8 + MAX_BOOK_NAME_BYTES;
    bytes[offset..offset + 4].copy_from_slice(&book.size().to_le_bytes());
    bytes[offset + 4..offset + 8].copy_from_slice(&(progress.spine_index() as u32).to_le_bytes());
    bytes[offset + 8..offset + 12].copy_from_slice(&(progress.page_index() as u32).to_le_bytes());
    bytes[offset + 12..offset + 16].copy_from_slice(&(progress.page_count() as u32).to_le_bytes());
    bytes[offset + 16..offset + 20].copy_from_slice(&progress.preferences().packed().to_le_bytes());
    let checksum = crc32fast::hash(&bytes[..CHECKSUM_OFFSET]);
    bytes[CHECKSUM_OFFSET..].copy_from_slice(&checksum.to_le_bytes());
    bytes
}

fn decode_bookmark(bytes: &[u8; BOOKMARK_BYTES]) -> Option<(BookIdentity, BookProgress)> {
    if u32::from_le_bytes(bytes[0..4].try_into().ok()?) != BOOKMARK_MAGIC
        || crc32fast::hash(&bytes[..CHECKSUM_OFFSET])
            != u32::from_le_bytes(bytes[CHECKSUM_OFFSET..].try_into().ok()?)
    {
        return None;
    }
    let name_length = usize::try_from(u32::from_le_bytes(bytes[4..8].try_into().ok()?)).ok()?;
    if name_length == 0 || name_length > MAX_BOOK_NAME_BYTES {
        return None;
    }
    let name =
        BookFileName::try_from(core::str::from_utf8(&bytes[8..8 + name_length]).ok()?).ok()?;
    let offset = 8 + MAX_BOOK_NAME_BYTES;
    let identity = BookIdentity::new(
        name,
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().ok()?),
    );
    let spines = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?);
    let pages = u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().ok()?);
    let counts = u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().ok()?);
    let preferences = crate::app::ReaderPreferences::from_packed(u32::from_le_bytes(
        bytes[offset + 16..offset + 20].try_into().ok()?,
    ))?;
    let progress = BookProgress::new(
        usize::try_from(spines).ok()?,
        usize::try_from(pages).ok()?,
        usize::try_from(counts).ok()?,
        preferences,
    )?;
    Some((identity, progress))
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
        let identity = BookIdentity::from(book);
        let names = BookmarkNames::new(identity);
        self.storage.with_app_directory(|app| {
            let result = match app.open_dir(BOOKMARK_DIRECTORY) {
                Ok(directory) => {
                    let read = (|| {
                        for name in [names.stored(), names.backup()] {
                            let Some(bytes) = Self::read_bookmark_file(&directory, name)? else {
                                continue;
                            };
                            if let Some((stored, progress)) = decode_bookmark(&bytes)
                                && stored == identity
                            {
                                return Ok(Some(progress));
                            }
                        }
                        Ok(None)
                    })();
                    match directory.close() {
                        Ok(()) => read,
                        Err(error) => Err(AppDataError::Filesystem(error)),
                    }
                }
                Err(Error::NotFound) => Ok(None),
                Err(error) => return Err(error),
            };
            Ok(result)
        })?
    }

    pub fn write_book_progress(
        &self,
        book: &BookFile,
        progress: BookProgress,
    ) -> Result<(), AppDataError<D::Error>> {
        let identity = BookIdentity::from(book);
        let names = BookmarkNames::new(identity);
        let bytes = encode_bookmark(book, progress);
        self.storage
            .ensure_layout()
            .map_err(AppDataError::Filesystem)?;
        self.storage.with_app_directory(|app| {
            let directory = app.open_dir(BOOKMARK_DIRECTORY)?;
            let result = Self::publish_bookmark(&directory, &names, &bytes);
            directory.close()?;
            Ok(result)
        })?
    }

    fn publish_bookmark(
        directory: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        names: &BookmarkNames,
        bytes: &[u8; BOOKMARK_BYTES],
    ) -> Result<(), AppDataError<D::Error>> {
        let written = Self::write_bookmark_file(directory, names.temp(), bytes);
        if written.is_err()
            || Self::read_bookmark_file(directory, names.temp())?.as_ref() != Some(bytes)
        {
            directory
                .delete_entry_in_dir(names.temp())
                .map_err(AppDataError::Filesystem)?;
            return Err(AppDataError::IncompleteWrite);
        }
        if let Some(existing) = Self::read_bookmark_file(directory, names.stored())? {
            Self::copy_bookmark(directory, names.stored(), names.backup())?;
            if Self::read_bookmark_file(directory, names.backup())? != Some(existing) {
                return Err(AppDataError::IncompleteWrite);
            }
        }
        match directory.delete_entry_in_dir(names.stored()) {
            Ok(()) | Err(Error::NotFound) => {}
            Err(error) => return Err(AppDataError::Filesystem(error)),
        }
        Self::copy_bookmark(directory, names.temp(), names.stored())?;
        if Self::read_bookmark_file(directory, names.stored())?.as_ref() != Some(bytes) {
            return Err(AppDataError::IncompleteWrite);
        }
        directory
            .delete_entry_in_dir(names.temp())
            .map_err(AppDataError::Filesystem)
    }

    fn write_bookmark_file(
        directory: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        name: &str,
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
        source: &str,
        target: &str,
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
        name: &str,
    ) -> Result<Option<[u8; BOOKMARK_BYTES]>, AppDataError<D::Error>> {
        let file = match directory.open_file_in_dir(name, Mode::ReadOnly) {
            Ok(file) => file,
            Err(Error::NotFound) => return Ok(None),
            Err(error) => return Err(AppDataError::Filesystem(error)),
        };
        if usize::try_from(file.length()).unwrap_or(usize::MAX) != BOOKMARK_BYTES {
            file.close().map_err(AppDataError::Filesystem)?;
            return Ok(None);
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
        Ok((offset == BOOKMARK_BYTES).then_some(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_and_decodes_a_record() {
        let book = BookFile::new(BookFileName::try_from("0C645338.EPB").unwrap(), 20_895_409);
        let preferences = crate::app::ReaderPreferences::default();
        let progress = BookProgress::new(9, 5, 15, preferences).unwrap();
        let bytes = encode_bookmark(&book, progress);
        match decode_bookmark(&bytes) {
            Some((identity, decoded)) => {
                assert_eq!(identity, BookIdentity::from(&book));
                assert_eq!(decoded, progress);
            }
            None => panic!("record did not decode; first bytes {:?}", &bytes[..16]),
        }
    }

    #[test]
    fn synthetic_slot_names_are_distinct_per_identity() {
        let atlas = BookmarkNames::new(BookIdentity::from(&BookFile::new(
            BookFileName::try_from("0C645338.EPB").unwrap(),
            20_895_409,
        )));
        let other = BookmarkNames::new(BookIdentity::from(&BookFile::new(
            BookFileName::try_from("0C645338.EPB").unwrap(),
            20_895_410,
        )));
        assert_eq!(atlas.stored().len(), 12);
        assert_eq!(atlas.stored().as_bytes()[9..], *b"BMK");
        assert_eq!(atlas.temp().as_bytes()[9..], *b"TMP");
        assert_ne!(atlas.stored(), other.stored());
    }
}
