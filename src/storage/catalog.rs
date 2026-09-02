use core::{fmt, ops::ControlFlow};

use embedded_sdmmc::{BlockDevice, Error, LfnBuffer, Mode, TimeSource, VolumeIdx, VolumeManager};
#[cfg(feature = "device-reader")]
use embedded_sdmmc::{File, Volume};

pub const BOOK_DIRECTORY: &str = "BOOKS";
pub const MAX_BOOK_NAME_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BookFileName {
    bytes: [u8; MAX_BOOK_NAME_BYTES],
    length: u16,
}

impl BookFileName {
    pub fn new(value: &str) -> Result<Self, BookFileNameError> {
        if value.is_empty() || value.len() > MAX_BOOK_NAME_BYTES {
            return Err(BookFileNameError);
        }
        let mut bytes = [0; MAX_BOOK_NAME_BYTES];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        Ok(Self {
            bytes,
            length: value.len() as u16,
        })
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..usize::from(self.length)])
            .expect("book filenames are copied from UTF-8 FAT names")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BookFileNameError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BookFile {
    name: BookFileName,
    size: u32,
}

impl BookFile {
    pub const fn name(&self) -> &BookFileName {
        &self.name
    }

    pub const fn size(&self) -> u32 {
        self.size
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookCatalog<const CAPACITY: usize> {
    books: [Option<BookFile>; CAPACITY],
    length: usize,
    unsupported_files: usize,
    skipped_names: usize,
    truncated: bool,
}

impl<const CAPACITY: usize> BookCatalog<CAPACITY> {
    pub const fn empty() -> Self {
        Self {
            books: [None; CAPACITY],
            length: 0,
            unsupported_files: 0,
            skipped_names: 0,
            truncated: false,
        }
    }

    pub fn books(&self) -> impl Iterator<Item = &BookFile> {
        self.books[..self.length].iter().flatten()
    }

    pub const fn len(&self) -> usize {
        self.length
    }

    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub const fn unsupported_files(&self) -> usize {
        self.unsupported_files
    }

    pub const fn skipped_names(&self) -> usize {
        self.skipped_names
    }

    pub const fn is_truncated(&self) -> bool {
        self.truncated
    }

    fn inspect(&mut self, name: &str, size: u32) {
        if !is_epub(name) {
            self.unsupported_files = self.unsupported_files.saturating_add(1);
            return;
        }
        let Ok(name) = BookFileName::new(name) else {
            self.skipped_names = self.skipped_names.saturating_add(1);
            return;
        };
        if self.length == CAPACITY {
            self.truncated = true;
            return;
        }
        self.books[self.length] = Some(BookFile { name, size });
        self.length += 1;
    }
}

pub struct ReadOnlyFatBookStore<
    D,
    T,
    const MAX_DIRS: usize = 3,
    const MAX_FILES: usize = 1,
    const MAX_VOLUMES: usize = 1,
> where
    D: BlockDevice,
    T: TimeSource,
{
    manager: VolumeManager<D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>,
}

#[cfg(feature = "device-reader")]
pub struct FatBookReader<
    'store,
    D,
    T,
    const MAX_DIRS: usize = 3,
    const MAX_FILES: usize = 1,
    const MAX_VOLUMES: usize = 1,
> where
    D: BlockDevice,
    T: TimeSource,
{
    file: File<'store, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>,
    _volume: Volume<'store, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>,
}

#[cfg(feature = "device-reader")]
impl<D, T, const MAX_DIRS: usize, const MAX_FILES: usize, const MAX_VOLUMES: usize>
    crate::zip_stream::ReadAt for FatBookReader<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>
where
    D: BlockDevice,
    T: TimeSource,
{
    type Error = Error<D::Error>;

    fn len(&self) -> u32 {
        self.file.length()
    }

    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        self.file.seek_from_start(offset)?;
        self.file.read(output)
    }
}

impl<D, T, const MAX_DIRS: usize, const MAX_FILES: usize, const MAX_VOLUMES: usize>
    ReadOnlyFatBookStore<D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>
where
    D: BlockDevice,
    T: TimeSource,
{
    pub fn new(device: D, time_source: T) -> Self {
        Self {
            manager: VolumeManager::new_with_limits(device, time_source, 7_000),
        }
    }

    pub fn scan<const CAPACITY: usize>(&self) -> Result<BookCatalog<CAPACITY>, Error<D::Error>> {
        let volume = self.manager.open_volume(VolumeIdx(0))?;
        let root = volume.open_root_dir()?;
        let books = match root.open_dir(BOOK_DIRECTORY) {
            Ok(books) => books,
            Err(Error::NotFound) => return Ok(BookCatalog::empty()),
            Err(error) => return Err(error),
        };
        let mut catalog = BookCatalog::empty();
        let mut storage = [0; 768];
        let mut lfn = LfnBuffer::new(&mut storage);
        books.iterate_dir_lfn(&mut lfn, |entry, long_name| {
            if entry.attributes.is_directory()
                || entry.attributes.is_volume()
                || entry.attributes.is_hidden()
                || entry.attributes.is_system()
            {
                return ControlFlow::Continue(());
            }
            let mut short_name = ShortName::new();
            let name = match long_name {
                Some(name) => name,
                None => {
                    if fmt::write(&mut short_name, format_args!("{}", entry.name)).is_err() {
                        return ControlFlow::Continue(());
                    }
                    short_name.as_str()
                }
            };
            catalog.inspect(name, entry.size);
            ControlFlow::Continue(())
        })?;
        Ok(catalog)
    }

    #[cfg(feature = "device-reader")]
    pub fn open_reader(
        &self,
        book: BookFile,
    ) -> Result<FatBookReader<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>, Error<D::Error>> {
        let volume = self.manager.open_volume(VolumeIdx(0))?;
        let root = volume.open_root_dir()?;
        let books = root.open_dir(BOOK_DIRECTORY)?;
        let file = books.open_long_name_file_in_dir(book.name().as_str(), Mode::ReadOnly)?;
        drop(books);
        drop(root);
        Ok(FatBookReader {
            file,
            _volume: volume,
        })
    }

    pub fn file_length(&self, name: &BookFileName) -> Result<u32, Error<D::Error>> {
        let volume = self.manager.open_volume(VolumeIdx(0))?;
        let root = volume.open_root_dir()?;
        let books = root.open_dir(BOOK_DIRECTORY)?;
        let file = books.open_long_name_file_in_dir(name.as_str(), Mode::ReadOnly)?;
        Ok(file.length())
    }

    pub fn read_at(
        &self,
        name: &BookFileName,
        offset: u32,
        output: &mut [u8],
    ) -> Result<usize, Error<D::Error>> {
        let volume = self.manager.open_volume(VolumeIdx(0))?;
        let root = volume.open_root_dir()?;
        let books = root.open_dir(BOOK_DIRECTORY)?;
        let file = books.open_long_name_file_in_dir(name.as_str(), Mode::ReadOnly)?;
        file.seek_from_start(offset)?;
        file.read(output)
    }

    pub fn with_device<R>(&self, function: impl FnOnce(&mut D) -> R) -> R {
        self.manager.device(function)
    }
}

fn is_epub(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("epub"))
}

struct ShortName {
    bytes: [u8; 13],
    length: usize,
}

impl ShortName {
    const fn new() -> Self {
        Self {
            bytes: [0; 13],
            length: 0,
        }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.length]).unwrap_or("")
    }
}

impl fmt::Write for ShortName {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let end = self.length.checked_add(value.len()).ok_or(fmt::Error)?;
        if end > self.bytes.len() {
            return Err(fmt::Error);
        }
        self.bytes[self.length..end].copy_from_slice(value.as_bytes());
        self.length = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::{format, vec::Vec};

    use super::{BookCatalog, BookFileName, MAX_BOOK_NAME_BYTES, is_epub};

    #[test]
    fn accepts_epub_extensions_case_insensitively() {
        assert!(is_epub("book.epub"));
        assert!(is_epub("BOOK.EPUB"));
        assert!(!is_epub("book.epub.zip"));
        assert!(!is_epub("epub"));
    }

    #[test]
    fn catalog_distinguishes_books_unsupported_files_and_capacity() {
        let mut catalog = BookCatalog::<1>::empty();
        catalog.inspect("first.epub", 42);
        catalog.inspect("notes.txt", 10);
        catalog.inspect("second.epub", 84);

        let books = catalog.books().collect::<Vec<_>>();
        assert_eq!(books[0].name().as_str(), "first.epub");
        assert_eq!(books[0].size(), 42);
        assert_eq!(catalog.unsupported_files(), 1);
        assert!(catalog.is_truncated());
    }

    #[test]
    fn filenames_are_bounded_without_truncation() {
        let valid = "a".repeat(MAX_BOOK_NAME_BYTES);
        assert_eq!(BookFileName::new(&valid).unwrap().as_str(), valid);
        assert!(BookFileName::new(&format!("{valid}x")).is_err());
    }
}
