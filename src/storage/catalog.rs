use core::{fmt, ops::ControlFlow};

#[cfg(feature = "device-reader")]
use crate::{
    app::AppPreferences,
    image_decoder::ImageFormat,
    transfer::{ImageName, MAX_IMAGE_BYTES, UploadRequest, UploadSink, UploadTarget},
};
#[cfg(feature = "device-reader")]
use core::cell::Cell;
#[cfg(feature = "device-reader")]
use crc32fast::Hasher;
use embedded_sdmmc::{BlockDevice, Error, LfnBuffer, Mode, TimeSource, VolumeIdx, VolumeManager};
#[cfg(feature = "device-reader")]
use embedded_sdmmc::{Directory, File, Volume};

pub const BOOK_DIRECTORY: &str = "books";
pub const FILE_DIRECTORY: &str = "files";
// `embedded-sdmmc` creates only 8.3 names. Migrate `brew` to `.brew` when writable LFN support is available.
pub const APP_DATA_DIRECTORY: &str = "brew";
pub const CACHE_DIRECTORY: &str = "cache";
pub const BOOKMARK_DIRECTORY: &str = "bookmark";
pub const MAX_BOOK_NAME_BYTES: usize = 256;
#[cfg(feature = "device-reader")]
pub const MAX_DEVICE_IMAGE_BYTES: usize = MAX_IMAGE_BYTES;

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

#[cfg(feature = "device-reader")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageFile {
    name: ImageName,
    size: u32,
}

#[cfg(feature = "device-reader")]
impl ImageFile {
    pub const fn name(&self) -> &ImageName {
        &self.name
    }

    pub const fn size(&self) -> u32 {
        self.size
    }

    pub const fn format(&self) -> ImageFormat {
        self.name.format()
    }
}

#[cfg(feature = "device-reader")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageCatalog<const CAPACITY: usize> {
    images: [Option<ImageFile>; CAPACITY],
    length: usize,
    truncated: bool,
}

#[cfg(feature = "device-reader")]
impl<const CAPACITY: usize> ImageCatalog<CAPACITY> {
    pub const fn empty() -> Self {
        Self {
            images: [None; CAPACITY],
            length: 0,
            truncated: false,
        }
    }

    pub fn images(&self) -> impl Iterator<Item = ImageFile> + '_ {
        self.images[..self.length].iter().flatten().copied()
    }

    pub const fn len(&self) -> usize {
        self.length
    }

    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub const fn get(&self, index: usize) -> Option<ImageFile> {
        if index < self.length {
            self.images[index]
        } else {
            None
        }
    }

    pub fn position(&self, name: ImageName) -> Option<usize> {
        self.images().position(|image| image.name == name)
    }

    pub const fn is_truncated(&self) -> bool {
        self.truncated
    }

    fn inspect(&mut self, name: &str, size: u32) {
        if size == 0 || size > MAX_DEVICE_IMAGE_BYTES as u32 {
            return;
        }
        let Ok(name) = ImageName::parse(name) else {
            return;
        };
        if self.length == CAPACITY {
            self.truncated = true;
            return;
        }
        let image = ImageFile { name, size };
        let mut index = self.length;
        while index > 0 && self.images[index - 1].is_some_and(|previous| previous.name > image.name)
        {
            self.images[index] = self.images[index - 1];
            index -= 1;
        }
        self.images[index] = Some(image);
        self.length += 1;
    }
}

pub struct FatStorage<
    D,
    T,
    const MAX_DIRS: usize = 3,
    const MAX_FILES: usize = 2,
    const MAX_VOLUMES: usize = 1,
> where
    D: BlockDevice,
    T: TimeSource,
{
    manager: VolumeManager<D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>,
}

#[cfg(feature = "device-reader")]
pub struct FatFileReader<
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
    next_offset: Cell<Option<u32>>,
    _volume: Volume<'store, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>,
}

#[cfg(feature = "device-reader")]
impl<D, T, const MAX_DIRS: usize, const MAX_FILES: usize, const MAX_VOLUMES: usize>
    crate::zip_stream::ReadAt for FatFileReader<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>
where
    D: BlockDevice,
    T: TimeSource,
{
    type Error = Error<D::Error>;

    fn len(&self) -> u32 {
        self.file.length()
    }

    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        if self.next_offset.get() != Some(offset) {
            self.next_offset.set(None);
            self.file.seek_from_start(offset)?;
        }
        self.next_offset.set(None);
        let count = self.file.read(output)?;
        self.next_offset.set(
            u32::try_from(count)
                .ok()
                .and_then(|count| offset.checked_add(count)),
        );
        Ok(count)
    }
}

impl<D, T, const MAX_DIRS: usize, const MAX_FILES: usize, const MAX_VOLUMES: usize>
    FatStorage<D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>
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
    ) -> Result<FatFileReader<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>, Error<D::Error>> {
        let volume = self.manager.open_volume(VolumeIdx(0))?;
        let root = volume.open_root_dir()?;
        let books = root.open_dir(BOOK_DIRECTORY)?;
        let file = books.open_long_name_file_in_dir(book.name().as_str(), Mode::ReadOnly)?;
        drop(books);
        drop(root);
        Ok(FatFileReader {
            file,
            next_offset: Cell::new(None),
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

    #[cfg(feature = "device-reader")]
    pub fn ensure_layout(&self) -> Result<(), Error<D::Error>> {
        let volume = self.manager.open_volume(VolumeIdx(0))?;
        let root = volume.open_root_dir()?;
        Self::ensure_directory(&root, APP_DATA_DIRECTORY)?;
        Self::ensure_directory(&root, BOOK_DIRECTORY)?;
        Self::ensure_directory(&root, FILE_DIRECTORY)?;
        let app = root.open_dir(APP_DATA_DIRECTORY)?;
        Self::ensure_directory(&app, CACHE_DIRECTORY)?;
        Self::ensure_directory(&app, BOOKMARK_DIRECTORY)?;
        Ok(())
    }

    #[cfg(feature = "device-reader")]
    pub const fn app_data(&self) -> AppDataStore<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES> {
        AppDataStore { storage: self }
    }

    #[cfg(feature = "device-reader")]
    fn ensure_directory(
        directory: &Directory<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>,
        name: &str,
    ) -> Result<(), Error<D::Error>> {
        match directory.open_dir(name) {
            Ok(child) => child.close(),
            Err(Error::NotFound) => directory.make_dir_in_dir(name),
            Err(error) => Err(error),
        }
    }

    #[cfg(feature = "device-reader")]
    fn with_app_directory<R>(
        &self,
        function: impl FnOnce(
            &Directory<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>,
        ) -> Result<R, Error<D::Error>>,
    ) -> Result<R, AppDataError<D::Error>> {
        self.with_directory(APP_DATA_DIRECTORY, function)
    }

    #[cfg(feature = "device-reader")]
    fn with_files_directory<R>(
        &self,
        function: impl FnOnce(
            &Directory<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>,
        ) -> Result<R, Error<D::Error>>,
    ) -> Result<R, AppDataError<D::Error>> {
        self.with_directory(FILE_DIRECTORY, function)
    }

    #[cfg(feature = "device-reader")]
    fn with_directory<R>(
        &self,
        name: &str,
        function: impl FnOnce(
            &Directory<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>,
        ) -> Result<R, Error<D::Error>>,
    ) -> Result<R, AppDataError<D::Error>> {
        let volume = self
            .manager
            .open_volume(VolumeIdx(0))
            .map_err(AppDataError::Filesystem)?;
        let root = volume.open_root_dir().map_err(AppDataError::Filesystem)?;
        let directory = root.open_dir(name).map_err(|error| match error {
            Error::NotFound => AppDataError::DirectoryMissing,
            error => AppDataError::Filesystem(error),
        })?;
        drop(root);
        let result = function(&directory).map_err(AppDataError::Filesystem);
        drop(directory);
        drop(volume);
        result
    }

    #[cfg(feature = "device-reader")]
    fn with_upload_directories<R>(
        &self,
        function: impl FnOnce(
            &Directory<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>,
            &Directory<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>,
        ) -> Result<R, Error<D::Error>>,
    ) -> Result<R, AppDataError<D::Error>> {
        let volume = self
            .manager
            .open_volume(VolumeIdx(0))
            .map_err(AppDataError::Filesystem)?;
        let root = volume.open_root_dir().map_err(AppDataError::Filesystem)?;
        let app = root
            .open_dir(APP_DATA_DIRECTORY)
            .map_err(AppDataError::Filesystem)?;
        let files = root
            .open_dir(FILE_DIRECTORY)
            .map_err(AppDataError::Filesystem)?;
        drop(root);
        let result = function(&app, &files).map_err(AppDataError::Filesystem);
        drop(files);
        drop(app);
        drop(volume);
        result
    }
}

#[cfg(feature = "device-reader")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoredImage {
    length: usize,
    format: ImageFormat,
}

#[cfg(feature = "device-reader")]
impl StoredImage {
    pub const fn length(self) -> usize {
        self.length
    }

    pub const fn format(self) -> ImageFormat {
        self.format
    }
}

#[cfg(feature = "device-reader")]
#[derive(Debug, Eq, PartialEq)]
pub enum AppDataError<E: core::error::Error> {
    Filesystem(Error<E>),
    DirectoryMissing,
    FileTooLarge,
    InvalidMetadata,
    ChecksumMismatch,
    IncompleteWrite,
    TargetExists,
}

#[cfg(feature = "device-reader")]
pub struct AppDataStore<
    'storage,
    D,
    T,
    const MAX_DIRS: usize,
    const MAX_FILES: usize,
    const MAX_VOLUMES: usize,
> where
    D: BlockDevice,
    T: TimeSource,
{
    storage: &'storage FatStorage<D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>,
}

#[cfg(feature = "device-reader")]
impl<D, T, const MAX_DIRS: usize, const MAX_FILES: usize, const MAX_VOLUMES: usize>
    AppDataStore<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>
where
    D: BlockDevice,
    T: TimeSource,
{
    pub fn scan_images<const CAPACITY: usize>(
        &self,
    ) -> Result<ImageCatalog<CAPACITY>, AppDataError<D::Error>> {
        self.recover_image_upload(&mut [0; 512])?;
        self.storage.with_files_directory(|directory| {
            let mut catalog = ImageCatalog::empty();
            let mut storage = [0; 768];
            let mut lfn = LfnBuffer::new(&mut storage);
            directory.iterate_dir_lfn(&mut lfn, |entry, long_name| {
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
        })
    }

    pub fn read_image(
        &self,
        name: ImageName,
        output: &mut [u8],
    ) -> Result<StoredImage, AppDataError<D::Error>> {
        let length = self.read_named_file(name.as_str(), output)?;
        if length == 0 || ImageFormat::detect(&output[..length]) != Some(name.format()) {
            return Err(AppDataError::InvalidMetadata);
        }
        Ok(StoredImage {
            length,
            format: name.format(),
        })
    }

    pub fn read_selected_image(&self) -> Option<ImageName> {
        for file in [
            AppDataFile::ImageSelection,
            AppDataFile::ImageSelectionBackup,
        ] {
            let mut bytes = [0; IMAGE_SELECTION_BYTES];
            if self.read_file(file, &mut bytes).ok() == Some(bytes.len())
                && let Some(name) = decode_image_selection(bytes)
            {
                return Some(name);
            }
        }
        None
    }

    pub fn write_selected_image(&self, name: ImageName) -> Result<(), AppDataError<D::Error>> {
        let bytes = encode_image_selection(name);
        self.write_file(AppDataFile::ImageSelectionTemp, &bytes)?;
        let mut readback = [0; IMAGE_SELECTION_BYTES];
        if self.read_file(AppDataFile::ImageSelectionTemp, &mut readback)? != bytes.len()
            || readback != bytes
        {
            return Err(AppDataError::IncompleteWrite);
        }
        if self.file_exists(AppDataFile::ImageSelection)? {
            self.delete_if_present(AppDataFile::ImageSelectionBackup)?;
            self.copy_file(
                AppDataFile::ImageSelection,
                AppDataFile::ImageSelectionBackup,
                &mut [0; 32],
            )?;
        }
        self.delete_if_present(AppDataFile::ImageSelection)?;
        self.copy_file(
            AppDataFile::ImageSelectionTemp,
            AppDataFile::ImageSelection,
            &mut [0; 32],
        )?;
        if self.read_selected_image() != Some(name) {
            return Err(AppDataError::IncompleteWrite);
        }
        self.delete_if_present(AppDataFile::ImageSelectionTemp)
    }

    pub fn read_preferences(&self) -> Result<Option<AppPreferences>, AppDataError<D::Error>> {
        for name in [AppDataFile::Preferences, AppDataFile::PreferencesBackup] {
            if let Some(preferences) = self.read_preferences_file(name) {
                return Ok(Some(preferences));
            }
        }
        Ok(None)
    }

    pub fn write_preferences(
        &self,
        preferences: AppPreferences,
    ) -> Result<(), AppDataError<D::Error>> {
        let packed = preferences.packed();
        let mut bytes = [0; 12];
        bytes[0..4].copy_from_slice(&PREFS_MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&packed.to_le_bytes());
        bytes[8..12].copy_from_slice(&preference_checksum(packed).to_le_bytes());
        self.write_file(AppDataFile::PreferencesTemp, &bytes)?;
        let mut readback = [0; 12];
        if self.read_file(AppDataFile::PreferencesTemp, &mut readback)? != bytes.len()
            || readback != bytes
        {
            self.delete_if_present(AppDataFile::PreferencesTemp)?;
            return Err(AppDataError::IncompleteWrite);
        }
        if self
            .read_preferences_file(AppDataFile::Preferences)
            .is_some()
        {
            self.delete_if_present(AppDataFile::PreferencesBackup)?;
            self.copy_file(
                AppDataFile::Preferences,
                AppDataFile::PreferencesBackup,
                &mut [0; 32],
            )?;
        }
        self.delete_if_present(AppDataFile::Preferences)?;
        self.copy_file(
            AppDataFile::PreferencesTemp,
            AppDataFile::Preferences,
            &mut [0; 32],
        )?;
        if self.read_preferences_file(AppDataFile::Preferences) != Some(preferences) {
            return Err(AppDataError::IncompleteWrite);
        }
        self.delete_if_present(AppDataFile::PreferencesTemp)?;
        Ok(())
    }

    pub fn begin_image_upload(&self, request: UploadRequest) -> Result<(), AppDataError<D::Error>> {
        let UploadTarget::Image(name) = request.target();
        self.storage
            .ensure_layout()
            .map_err(AppDataError::Filesystem)?;
        self.recover_image_upload(&mut [0; 512])?;
        if self.named_file_exists(name.as_str())?
            && self
                .verify_named_file(name, request.length(), request.crc32(), &mut [0; 512])
                .is_err()
        {
            return Err(AppDataError::TargetExists);
        }
        self.delete_if_present(AppDataFile::ImageUploadTemp)?;
        self.write_file(AppDataFile::ImageUploadTemp, &[])
    }

    pub fn append_image_upload(&self, bytes: &[u8]) -> Result<(), AppDataError<D::Error>> {
        self.append_file(AppDataFile::ImageUploadTemp, bytes)
    }

    pub fn abort_image_upload(&self) -> Result<(), AppDataError<D::Error>> {
        self.recover_image_upload(&mut [0; 512])
    }

    pub fn commit_image_upload(
        &self,
        request: UploadRequest,
        scratch: &mut [u8],
    ) -> Result<(), AppDataError<D::Error>> {
        let UploadTarget::Image(name) = request.target();
        self.verify_file(
            AppDataFile::ImageUploadTemp,
            request.length(),
            request.crc32(),
            scratch,
        )?;
        if self.named_file_exists(name.as_str())? {
            self.verify_named_file(name, request.length(), request.crc32(), scratch)?;
            self.delete_if_present(AppDataFile::ImageUploadTemp)?;
            return Ok(());
        }
        let transaction = ImageUploadRecord {
            name,
            length: request.length(),
            crc32: request.crc32(),
        };
        self.write_file(AppDataFile::ImageUploadTransaction, &transaction.encode())?;
        let mut transaction_readback = [0; IMAGE_UPLOAD_RECORD_BYTES];
        if self.read_file(
            AppDataFile::ImageUploadTransaction,
            &mut transaction_readback,
        )? != transaction_readback.len()
            || ImageUploadRecord::decode(transaction_readback) != Some(transaction)
        {
            return Err(AppDataError::IncompleteWrite);
        }
        self.copy_file_to_named(AppDataFile::ImageUploadTemp, name.as_str(), scratch)?;
        self.verify_named_file(name, request.length(), request.crc32(), scratch)?;
        self.delete_if_present(AppDataFile::ImageUploadTransaction)?;
        self.delete_if_present(AppDataFile::ImageUploadTemp)?;
        Ok(())
    }

    fn recover_image_upload(&self, scratch: &mut [u8]) -> Result<(), AppDataError<D::Error>> {
        if !self.file_exists(AppDataFile::ImageUploadTransaction)? {
            self.delete_if_present(AppDataFile::ImageUploadTemp)?;
            return Ok(());
        }
        let mut bytes = [0; IMAGE_UPLOAD_RECORD_BYTES];
        let transaction = self
            .read_file(AppDataFile::ImageUploadTransaction, &mut bytes)
            .ok()
            .filter(|length| *length == bytes.len())
            .and_then(|_| ImageUploadRecord::decode(bytes));
        if let Some(transaction) = transaction
            && self
                .verify_named_file(
                    transaction.name,
                    transaction.length,
                    transaction.crc32,
                    scratch,
                )
                .is_err()
        {
            self.delete_named_if_present(transaction.name.as_str())?;
        }
        self.delete_if_present(AppDataFile::ImageUploadTransaction)?;
        self.delete_if_present(AppDataFile::ImageUploadTemp)
    }

    fn read_preferences_file(&self, name: AppDataFile) -> Option<AppPreferences> {
        let mut bytes = [0; 12];
        let length = self.read_file(name, &mut bytes).ok()?;
        if length != bytes.len() || u32::from_le_bytes(bytes[0..4].try_into().ok()?) != PREFS_MAGIC
        {
            return None;
        }
        let packed = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
        let checksum = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
        (preference_checksum(packed) == checksum)
            .then(|| AppPreferences::from_packed(packed))
            .flatten()
    }

    fn verify_file(
        &self,
        file: AppDataFile,
        expected_length: usize,
        expected_crc32: u32,
        scratch: &mut [u8],
    ) -> Result<(), AppDataError<D::Error>> {
        let (length, crc32) = self.file_digest(file, scratch)?;
        if length != expected_length {
            return Err(AppDataError::IncompleteWrite);
        }
        if crc32 != expected_crc32 {
            return Err(AppDataError::ChecksumMismatch);
        }
        Ok(())
    }

    fn file_digest(
        &self,
        file: AppDataFile,
        scratch: &mut [u8],
    ) -> Result<(usize, u32), AppDataError<D::Error>> {
        if scratch.is_empty() {
            return Err(AppDataError::IncompleteWrite);
        }
        self.storage.with_app_directory(|directory| {
            let source = directory.open_file_in_dir(file.name(), Mode::ReadOnly)?;
            let mut length = 0usize;
            let mut hasher = Hasher::new();
            while !source.is_eof() {
                let count = source.read(scratch)?;
                if count == 0 {
                    break;
                }
                length = length.saturating_add(count);
                hasher.update(&scratch[..count]);
            }
            source.close()?;
            Ok((length, hasher.finalize()))
        })
    }

    fn read_file(
        &self,
        file: AppDataFile,
        output: &mut [u8],
    ) -> Result<usize, AppDataError<D::Error>> {
        self.storage.with_app_directory(|directory| {
            let source = directory.open_file_in_dir(file.name(), Mode::ReadOnly)?;
            let length = usize::try_from(source.length()).unwrap_or(usize::MAX);
            if length > output.len() {
                return Err(Error::NotEnoughSpace);
            }
            let mut offset = 0usize;
            while offset < length {
                let count = source.read(&mut output[offset..length])?;
                if count == 0 {
                    break;
                }
                offset += count;
            }
            source.close()?;
            Ok(offset)
        })
    }

    fn read_named_file(
        &self,
        name: &str,
        output: &mut [u8],
    ) -> Result<usize, AppDataError<D::Error>> {
        self.storage.with_files_directory(|directory| {
            let source = directory.open_file_in_dir(name, Mode::ReadOnly)?;
            let length = usize::try_from(source.length()).unwrap_or(usize::MAX);
            if length > output.len() {
                return Err(Error::NotEnoughSpace);
            }
            let mut offset = 0usize;
            while offset < length {
                let count = source.read(&mut output[offset..length])?;
                if count == 0 {
                    break;
                }
                offset += count;
            }
            source.close()?;
            Ok(offset)
        })
    }

    fn verify_named_file(
        &self,
        name: ImageName,
        expected_length: usize,
        expected_crc32: u32,
        scratch: &mut [u8],
    ) -> Result<(), AppDataError<D::Error>> {
        let (length, crc32) = self.file_digest_named(name.as_str(), scratch)?;
        if length != expected_length {
            return Err(AppDataError::IncompleteWrite);
        }
        if crc32 != expected_crc32 {
            return Err(AppDataError::ChecksumMismatch);
        }
        let mut signature = [0; 8];
        let signature_length = self.read_named_prefix(name.as_str(), &mut signature)?;
        if ImageFormat::detect(&signature[..signature_length]) != Some(name.format()) {
            return Err(AppDataError::InvalidMetadata);
        }
        Ok(())
    }

    fn read_named_prefix(
        &self,
        name: &str,
        output: &mut [u8],
    ) -> Result<usize, AppDataError<D::Error>> {
        self.storage.with_files_directory(|directory| {
            let source = directory.open_file_in_dir(name, Mode::ReadOnly)?;
            let length = source.read(output)?;
            source.close()?;
            Ok(length)
        })
    }

    fn file_digest_named(
        &self,
        name: &str,
        scratch: &mut [u8],
    ) -> Result<(usize, u32), AppDataError<D::Error>> {
        if scratch.is_empty() {
            return Err(AppDataError::IncompleteWrite);
        }
        self.storage.with_files_directory(|directory| {
            let source = directory.open_file_in_dir(name, Mode::ReadOnly)?;
            let mut length = 0usize;
            let mut hasher = Hasher::new();
            while !source.is_eof() {
                let count = source.read(scratch)?;
                if count == 0 {
                    break;
                }
                length = length.saturating_add(count);
                hasher.update(&scratch[..count]);
            }
            source.close()?;
            Ok((length, hasher.finalize()))
        })
    }

    fn write_file(&self, file: AppDataFile, bytes: &[u8]) -> Result<(), AppDataError<D::Error>> {
        self.storage.with_app_directory(|directory| {
            let target =
                directory.open_file_in_dir(file.name(), Mode::ReadWriteCreateOrTruncate)?;
            target.write(bytes)?;
            target.close()
        })
    }

    fn append_file(&self, file: AppDataFile, bytes: &[u8]) -> Result<(), AppDataError<D::Error>> {
        self.storage.with_app_directory(|directory| {
            let target = directory.open_file_in_dir(file.name(), Mode::ReadWriteCreateOrAppend)?;
            target.write(bytes)?;
            target.close()
        })
    }

    fn copy_file(
        &self,
        source: AppDataFile,
        target: AppDataFile,
        scratch: &mut [u8],
    ) -> Result<(), AppDataError<D::Error>> {
        if scratch.is_empty() {
            return Err(AppDataError::IncompleteWrite);
        }
        self.storage.with_app_directory(|directory| {
            let source = directory.open_file_in_dir(source.name(), Mode::ReadOnly)?;
            let target =
                directory.open_file_in_dir(target.name(), Mode::ReadWriteCreateOrTruncate)?;
            while !source.is_eof() {
                let count = source.read(scratch)?;
                if count == 0 {
                    break;
                }
                target.write(&scratch[..count])?;
            }
            target.close()?;
            source.close()
        })
    }

    fn copy_file_to_named(
        &self,
        source: AppDataFile,
        target: &str,
        scratch: &mut [u8],
    ) -> Result<(), AppDataError<D::Error>> {
        if scratch.is_empty() {
            return Err(AppDataError::IncompleteWrite);
        }
        self.storage.with_upload_directories(|app, files| {
            let source = app.open_file_in_dir(source.name(), Mode::ReadOnly)?;
            let target = files.open_file_in_dir(target, Mode::ReadWriteCreateOrTruncate)?;
            while !source.is_eof() {
                let count = source.read(scratch)?;
                if count == 0 {
                    break;
                }
                target.write(&scratch[..count])?;
            }
            target.close()?;
            source.close()
        })
    }

    fn named_file_exists(&self, name: &str) -> Result<bool, AppDataError<D::Error>> {
        self.storage
            .with_files_directory(|directory| Ok(directory.directory_entry_exists(name)))
    }

    fn delete_named_if_present(&self, name: &str) -> Result<(), AppDataError<D::Error>> {
        self.storage.with_files_directory(|directory| {
            if directory.directory_entry_exists(name) {
                directory.delete_entry_in_dir(name)?;
            }
            Ok(())
        })
    }

    fn file_exists(&self, file: AppDataFile) -> Result<bool, AppDataError<D::Error>> {
        self.storage
            .with_app_directory(|directory| Ok(directory.directory_entry_exists(file.name())))
    }

    fn delete_if_present(&self, file: AppDataFile) -> Result<(), AppDataError<D::Error>> {
        self.storage.with_app_directory(|directory| {
            if directory.directory_entry_exists(file.name()) {
                directory.delete_entry_in_dir(file.name())?;
            }
            Ok(())
        })
    }
}

#[cfg(feature = "device-reader")]
#[derive(Clone, Copy)]
enum AppDataFile {
    Preferences,
    PreferencesTemp,
    PreferencesBackup,
    ImageSelection,
    ImageSelectionTemp,
    ImageSelectionBackup,
    ImageUploadTemp,
    ImageUploadTransaction,
}

#[cfg(feature = "device-reader")]
impl AppDataFile {
    const fn name(self) -> &'static str {
        match self {
            Self::Preferences => "PREFS.BIN",
            Self::PreferencesTemp => "PREFS.TMP",
            Self::PreferencesBackup => "PREFS.BAK",
            Self::ImageSelection => "SELECT.BIN",
            Self::ImageSelectionTemp => "SELECT.TMP",
            Self::ImageSelectionBackup => "SELECT.BAK",
            Self::ImageUploadTemp => "UPLOAD.TMP",
            Self::ImageUploadTransaction => "UPLOAD.TXN",
        }
    }
}

#[cfg(feature = "device-reader")]
const PREFS_MAGIC: u32 = 0x4254_5031;
#[cfg(feature = "device-reader")]
const IMAGE_SELECTION_MAGIC: u32 = 0x4254_5331;
#[cfg(feature = "device-reader")]
const IMAGE_SELECTION_BYTES: usize = 20;
#[cfg(feature = "device-reader")]
const IMAGE_UPLOAD_MAGIC: u32 = 0x4254_5531;
#[cfg(feature = "device-reader")]
const IMAGE_UPLOAD_RECORD_BYTES: usize = 28;

#[cfg(feature = "device-reader")]
fn encode_image_selection(name: ImageName) -> [u8; IMAGE_SELECTION_BYTES] {
    let mut bytes = [0; IMAGE_SELECTION_BYTES];
    bytes[0..4].copy_from_slice(&IMAGE_SELECTION_MAGIC.to_le_bytes());
    name.write_padded(
        (&mut bytes[4..16])
            .try_into()
            .expect("selection name slice is fixed"),
    );
    let checksum = crc32fast::hash(&bytes[..16]);
    bytes[16..20].copy_from_slice(&checksum.to_le_bytes());
    bytes
}

#[cfg(feature = "device-reader")]
fn decode_image_selection(bytes: [u8; IMAGE_SELECTION_BYTES]) -> Option<ImageName> {
    if u32::from_le_bytes(bytes[0..4].try_into().ok()?) != IMAGE_SELECTION_MAGIC
        || crc32fast::hash(&bytes[..16]) != u32::from_le_bytes(bytes[16..20].try_into().ok()?)
    {
        return None;
    }
    ImageName::from_padded(bytes[4..16].try_into().ok()?).ok()
}

#[cfg(feature = "device-reader")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ImageUploadRecord {
    name: ImageName,
    length: usize,
    crc32: u32,
}

#[cfg(feature = "device-reader")]
impl ImageUploadRecord {
    fn encode(self) -> [u8; IMAGE_UPLOAD_RECORD_BYTES] {
        let mut bytes = [0; IMAGE_UPLOAD_RECORD_BYTES];
        bytes[0..4].copy_from_slice(&IMAGE_UPLOAD_MAGIC.to_le_bytes());
        self.name.write_padded(
            (&mut bytes[4..16])
                .try_into()
                .expect("upload name slice is fixed"),
        );
        bytes[16..20].copy_from_slice(
            &u32::try_from(self.length)
                .expect("image upload length is bounded")
                .to_le_bytes(),
        );
        bytes[20..24].copy_from_slice(&self.crc32.to_le_bytes());
        let checksum = crc32fast::hash(&bytes[..24]);
        bytes[24..28].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }

    fn decode(bytes: [u8; IMAGE_UPLOAD_RECORD_BYTES]) -> Option<Self> {
        if u32::from_le_bytes(bytes[0..4].try_into().ok()?) != IMAGE_UPLOAD_MAGIC
            || crc32fast::hash(&bytes[..24]) != u32::from_le_bytes(bytes[24..28].try_into().ok()?)
        {
            return None;
        }
        let length = usize::try_from(u32::from_le_bytes(bytes[16..20].try_into().ok()?)).ok()?;
        if length == 0 || length > MAX_DEVICE_IMAGE_BYTES {
            return None;
        }
        Some(Self {
            name: ImageName::from_padded(bytes[4..16].try_into().ok()?).ok()?,
            length,
            crc32: u32::from_le_bytes(bytes[20..24].try_into().ok()?),
        })
    }
}

#[cfg(feature = "device-reader")]
fn preference_checksum(packed: u32) -> u32 {
    [PREFS_MAGIC, packed]
        .into_iter()
        .fold(0x811C_9DC5, |hash, word| {
            (hash ^ word).wrapping_mul(0x0100_0193)
        })
}

#[cfg(feature = "device-reader")]
impl<D, T, const MAX_DIRS: usize, const MAX_FILES: usize, const MAX_VOLUMES: usize> UploadSink
    for AppDataStore<'_, D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>
where
    D: BlockDevice,
    T: TimeSource,
{
    type Error = AppDataError<D::Error>;

    fn begin(&mut self, request: UploadRequest) -> Result<(), Self::Error> {
        self.begin_image_upload(request)
    }

    fn append(&mut self, _request: UploadRequest, bytes: &[u8]) -> Result<(), Self::Error> {
        self.append_image_upload(bytes)
    }

    fn commit(&mut self, request: UploadRequest, scratch: &mut [u8]) -> Result<(), Self::Error> {
        self.commit_image_upload(request, scratch)
    }

    fn abort(&mut self, _request: UploadRequest) -> Result<(), Self::Error> {
        self.abort_image_upload()
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

    use super::{
        APP_DATA_DIRECTORY, BOOK_DIRECTORY, BOOKMARK_DIRECTORY, BookCatalog, BookFileName,
        CACHE_DIRECTORY, FILE_DIRECTORY, MAX_BOOK_NAME_BYTES, is_epub,
    };
    #[cfg(feature = "device-reader")]
    use super::{
        ImageCatalog, ImageUploadRecord, MAX_DEVICE_IMAGE_BYTES, decode_image_selection,
        encode_image_selection,
    };
    #[cfg(feature = "device-reader")]
    use crate::transfer::ImageName;
    use embedded_sdmmc::ShortFileName;

    #[test]
    fn writable_directory_layout_uses_fat_short_names() {
        for name in [
            APP_DATA_DIRECTORY,
            CACHE_DIRECTORY,
            BOOKMARK_DIRECTORY,
            BOOK_DIRECTORY,
            FILE_DIRECTORY,
        ] {
            assert!(ShortFileName::create_from_str(name).is_ok(), "{name}");
        }
        assert!(ShortFileName::create_from_str(".brew").is_err());
    }

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

    #[cfg(feature = "device-reader")]
    #[test]
    fn image_records_round_trip_and_reject_corruption() {
        let name = ImageName::parse("NICE.PNG").unwrap();
        let mut selection = encode_image_selection(name);
        assert_eq!(decode_image_selection(selection), Some(name));
        selection[8] ^= 1;
        assert_eq!(decode_image_selection(selection), None);

        let record = ImageUploadRecord {
            name,
            length: MAX_DEVICE_IMAGE_BYTES,
            crc32: 0x1234_5678,
        };
        let mut encoded = record.encode();
        assert_eq!(ImageUploadRecord::decode(encoded), Some(record));
        encoded[20] ^= 1;
        assert_eq!(ImageUploadRecord::decode(encoded), None);
    }

    #[cfg(feature = "device-reader")]
    #[test]
    fn image_catalog_is_bounded_and_sorted() {
        let mut catalog = ImageCatalog::<2>::empty();
        catalog.inspect("MOOD.PNG", 20);
        catalog.inspect("AYA.JPG", 10);
        catalog.inspect("notes.txt", 30);
        catalog.inspect("NICE.PNG", 40);
        let images = catalog.images().collect::<Vec<_>>();
        assert_eq!(images[0].name().as_str(), "AYA.JPG");
        assert_eq!(images[1].name().as_str(), "MOOD.PNG");
        assert!(catalog.is_truncated());
    }

    #[test]
    fn filenames_are_bounded_without_truncation() {
        let valid = "a".repeat(MAX_BOOK_NAME_BYTES);
        assert_eq!(BookFileName::new(&valid).unwrap().as_str(), valid);
        assert!(BookFileName::new(&format!("{valid}x")).is_err());
    }
}
