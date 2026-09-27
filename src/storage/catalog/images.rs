use super::*;
use crate::{
    image_cache::{
        CACHE_HEADER_BYTES, CacheSlot, CacheState, ImageKey, ImageResource, ImageSource, ImageSpec,
        ImageWorkspace, PreparedImage,
    },
    image_decoder::stream::{self, MAX_IMAGE_FILE_BYTES},
    zip_stream::{ReadAt, StreamingZip, ZipValidationScratch},
};

const IMAGE_TEMP: &str = "IMAGE.TMP";
const CACHE_CURSOR: &str = "CLOCK.BIN";
const MAX_CACHE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 512;

pub(super) struct FileSource<
    'file,
    'store,
    D,
    T,
    const DIRS: usize,
    const FILES: usize,
    const VOLUMES: usize,
> where
    D: BlockDevice,
    T: TimeSource,
{
    file: &'file File<'store, D, T, DIRS, FILES, VOLUMES>,
    next_offset: Cell<Option<u32>>,
}

impl<
    'file,
    'store,
    D: BlockDevice,
    T: TimeSource,
    const DIRS: usize,
    const FILES: usize,
    const VOLUMES: usize,
> FileSource<'file, 'store, D, T, DIRS, FILES, VOLUMES>
{
    pub(super) fn new(file: &'file File<'store, D, T, DIRS, FILES, VOLUMES>) -> Self {
        Self {
            file,
            next_offset: Cell::new(None),
        }
    }
}

impl<D: BlockDevice, T: TimeSource, const DIRS: usize, const FILES: usize, const VOLUMES: usize>
    ReadAt for FileSource<'_, '_, D, T, DIRS, FILES, VOLUMES>
{
    type Error = Error<D::Error>;
    fn len(&self) -> u32 {
        self.file.length()
    }
    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        if self.next_offset.get() != Some(offset) {
            self.file.seek_from_start(offset)?;
        }
        self.next_offset.set(None);
        let count = self.file.read(output)?;
        self.next_offset.set(Some(offset + count as u32));
        Ok(count)
    }
}

impl<D, T, const DIRS: usize, const FILES: usize, const VOLUMES: usize>
    AppDataStore<'_, D, T, DIRS, FILES, VOLUMES>
where
    D: BlockDevice,
    T: TimeSource,
{
    pub fn probe_book_image(
        &self,
        book: BookFile,
        path: &str,
        workspace: &mut ImageWorkspace,
        zip: &mut ZipValidationScratch,
    ) -> Result<ImageResource, AppDataError<D::Error>> {
        let reader = self
            .storage
            .open_reader(book)
            .map_err(AppDataError::Filesystem)?;
        let archive = StreamingZip::open(reader, zip).map_err(|_| AppDataError::InvalidMetadata)?;
        workspace
            .probe_resource(&archive, path)
            .map_err(AppDataError::Image)
    }

    pub fn prepare_image(
        &self,
        source: ImageSource<'_>,
        spec: ImageSpec,
        workspace: &mut ImageWorkspace,
        zip: &mut ZipValidationScratch,
        output: &mut [u8],
    ) -> Result<PreparedImage, AppDataError<D::Error>> {
        self.prepare_pinned_image(source, spec, workspace, zip, output, &[])
    }

    pub fn prepare_pinned_image(
        &self,
        source: ImageSource<'_>,
        spec: ImageSpec,
        workspace: &mut ImageWorkspace,
        zip: &mut ZipValidationScratch,
        output: &mut [u8],
        protected: &[CacheSlot],
    ) -> Result<PreparedImage, AppDataError<D::Error>> {
        if output.len() < spec.byte_len() {
            return Err(AppDataError::FileTooLarge);
        }
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
        let source_directory = root
            .open_dir(match source {
                ImageSource::Book { .. } => BOOK_DIRECTORY,
                ImageSource::File(_) => FILE_DIRECTORY,
            })
            .map_err(AppDataError::Filesystem)?;
        root.close().map_err(AppDataError::Filesystem)?;
        let result = match source {
            ImageSource::Book { file: book, path } => {
                let file = FatStorage::open_book_file(&source_directory, book.name())
                    .map_err(AppDataError::Filesystem)?;
                let archive = StreamingZip::open(FileSource::new(&file), zip)
                    .map_err(|_| AppDataError::InvalidMetadata)?;
                let entry = archive
                    .find(path)
                    .map_err(|_| AppDataError::InvalidMetadata)?;
                if entry.uncompressed_size() == 0
                    || entry.uncompressed_size() > MAX_IMAGE_FILE_BYTES
                {
                    return Err(AppDataError::FileTooLarge);
                }
                let key = ImageKey::book(book, entry, spec).ok_or(AppDataError::InvalidMetadata)?;
                if let Some(image) = Self::read_cache(&cache, &key, output)? {
                    Ok(image)
                } else {
                    let staged = cache
                        .open_file_in_dir(IMAGE_TEMP, Mode::ReadWriteCreateOrTruncate)
                        .map_err(AppDataError::Filesystem)?;
                    let count = output.len().min(4096);
                    let extracted = archive.read_entry_to(
                        entry,
                        workspace.inflate(),
                        &mut output[..count],
                        |bytes| staged.write(bytes),
                    );
                    staged.close().map_err(AppDataError::Filesystem)?;
                    extracted.map_err(|_| AppDataError::InvalidMetadata)?;
                    file.close().map_err(AppDataError::Filesystem)?;
                    let staged = cache
                        .open_file_in_dir(IMAGE_TEMP, Mode::ReadOnly)
                        .map_err(AppDataError::Filesystem)?;
                    let decoded = Self::decode_and_cache(
                        &cache,
                        &FileSource::new(&staged),
                        key,
                        workspace,
                        output,
                        protected,
                    );
                    staged.close().map_err(AppDataError::Filesystem)?;
                    cache
                        .delete_entry_in_dir(IMAGE_TEMP)
                        .map_err(AppDataError::Filesystem)?;
                    decoded
                }
            }
            ImageSource::File(image) => {
                let file = source_directory
                    .open_file_in_dir(image.name().as_str(), Mode::ReadOnly)
                    .map_err(AppDataError::Filesystem)?;
                let reader = FileSource::new(&file);
                if file.length() == 0 || file.length() > MAX_IMAGE_FILE_BYTES {
                    return Err(AppDataError::FileTooLarge);
                }
                let mut checksum = Hasher::new();
                let mut offset = 0;
                let count = output.len().min(4096);
                while offset < file.length() {
                    let read = reader
                        .read_at(offset, &mut output[..count])
                        .map_err(AppDataError::Filesystem)?;
                    if read == 0 {
                        return Err(AppDataError::IncompleteWrite);
                    }
                    checksum.update(&output[..read]);
                    offset += read as u32;
                }
                let key = ImageKey::file(
                    image.name().as_str(),
                    file.length(),
                    checksum.finalize(),
                    spec,
                )
                .ok_or(AppDataError::InvalidMetadata)?;
                match Self::read_cache(&cache, &key, output)? {
                    Some(image) => Ok(image),
                    None => {
                        Self::decode_and_cache(&cache, &reader, key, workspace, output, protected)
                    }
                }
            }
        };
        source_directory
            .close()
            .and(cache.close())
            .and(volume.close())
            .map_err(AppDataError::Filesystem)?;
        result
    }

    pub fn read_prepared_image(
        &self,
        key: &ImageKey,
        output: &mut [u8],
    ) -> Result<(), AppDataError<D::Error>> {
        self.storage
            .with_app_directory(|app| {
                let cache = app.open_dir(CACHE_DIRECTORY)?;
                let result = Self::read_cache(&cache, key, output);
                cache.close()?;
                Ok(result)
            })??
            .ok_or(AppDataError::InvalidMetadata)?;
        Ok(())
    }

    fn read_cache(
        directory: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        key: &ImageKey,
        output: &mut [u8],
    ) -> Result<Option<PreparedImage>, AppDataError<D::Error>> {
        let file = match directory.open_file_in_dir(key.file_name().as_str(), Mode::ReadOnly) {
            Ok(file) => file,
            Err(Error::NotFound) => return Ok(None),
            Err(error) => return Err(AppDataError::Filesystem(error)),
        };
        let result = Self::read_cache_file(&file, key, output);
        file.close().map_err(AppDataError::Filesystem)?;
        result
    }

    fn read_cache_file(
        file: &File<'_, D, T, DIRS, FILES, VOLUMES>,
        key: &ImageKey,
        output: &mut [u8],
    ) -> Result<Option<PreparedImage>, AppDataError<D::Error>> {
        let length = key.spec().byte_len();
        if file.length() as usize != CACHE_HEADER_BYTES + length || output.len() < length {
            return Ok(None);
        }
        let mut header = [0; CACHE_HEADER_BYTES];
        Self::read_exact_image(file, &mut header)?;
        Self::read_exact_image(file, &mut output[..length])?;
        Ok(PreparedImage::decode(&header, key, &output[..length]))
    }

    fn read_exact_image(
        file: &File<'_, D, T, DIRS, FILES, VOLUMES>,
        mut output: &mut [u8],
    ) -> Result<(), AppDataError<D::Error>> {
        while !output.is_empty() {
            let count = file.read(output).map_err(AppDataError::Filesystem)?;
            if count == 0 {
                return Err(AppDataError::IncompleteWrite);
            }
            output = &mut output[count..];
        }
        Ok(())
    }

    fn reserve_cache(
        directory: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        name: &str,
        bytes: usize,
        protected: &[CacheSlot],
    ) -> Result<(), AppDataError<D::Error>> {
        let mut cursor = match directory.open_file_in_dir(CACHE_CURSOR, Mode::ReadOnly) {
            Ok(file) => {
                let mut record = [0; 8];
                let valid = file.length() == 8;
                if valid {
                    Self::read_exact_image(&file, &mut record)?;
                }
                file.close().map_err(AppDataError::Filesystem)?;
                if valid
                    && crc32fast::hash(&record[..4])
                        == u32::from_le_bytes(record[4..].try_into().unwrap())
                {
                    u32::from_le_bytes(record[..4].try_into().unwrap())
                } else {
                    0
                }
            }
            Err(Error::NotFound) => 0,
            Err(error) => return Err(AppDataError::Filesystem(error)),
        };
        loop {
            let mut total = bytes as u64;
            let mut count = 1;
            let mut first: Option<(u32, embedded_sdmmc::ShortFileName)> = None;
            let mut next: Option<(u32, embedded_sdmmc::ShortFileName)> = None;
            directory
                .iterate_dir(|entry| {
                    if entry.attributes.is_directory()
                        || entry.name.extension() != b"IMG"
                        || entry.name.base_name().len() != 8
                        || entry.name.base_name() == &name.as_bytes()[..8]
                    {
                        return ControlFlow::Continue(());
                    }
                    let Ok(base) = core::str::from_utf8(entry.name.base_name()) else {
                        return ControlFlow::Continue(());
                    };
                    let Ok(hash) = u32::from_str_radix(base, 16) else {
                        return ControlFlow::Continue(());
                    };
                    total += u64::from(entry.size);
                    count += 1;
                    if protected.contains(&CacheSlot(hash)) {
                        return ControlFlow::Continue(());
                    }
                    if first.is_none_or(|(old, _)| hash < old) {
                        first = Some((hash, entry.name));
                    }
                    if hash > cursor && next.is_none_or(|(old, _)| hash < old) {
                        next = Some((hash, entry.name));
                    }
                    ControlFlow::Continue(())
                })
                .map_err(AppDataError::Filesystem)?;
            if total <= MAX_CACHE_BYTES && count <= MAX_CACHE_ENTRIES {
                return Ok(());
            }
            let (hash, victim) = next.or(first).ok_or(AppDataError::FileTooLarge)?;
            directory
                .delete_entry_in_dir(victim)
                .map_err(AppDataError::Filesystem)?;
            cursor = hash;
            let file = directory
                .open_file_in_dir(CACHE_CURSOR, Mode::ReadWriteCreateOrTruncate)
                .map_err(AppDataError::Filesystem)?;
            let mut record = [0; 8];
            record[..4].copy_from_slice(&cursor.to_le_bytes());
            let checksum = crc32fast::hash(&record[..4]);
            record[4..].copy_from_slice(&checksum.to_le_bytes());
            file.write(&record).map_err(AppDataError::Filesystem)?;
            file.close().map_err(AppDataError::Filesystem)?;
        }
    }

    fn decode_and_cache<R: ReadAt>(
        directory: &Directory<'_, D, T, DIRS, FILES, VOLUMES>,
        source: &R,
        key: ImageKey,
        workspace: &mut ImageWorkspace,
        output: &mut [u8],
        protected: &[CacheSlot],
    ) -> Result<PreparedImage, AppDataError<D::Error>> {
        let spec = key.spec();
        stream::format(source).map_err(AppDataError::Image)?;
        let pixels = &mut output[..spec.byte_len()];
        let size = workspace
            .decode(source, spec, pixels)
            .map_err(AppDataError::Image)?;
        let image = PreparedImage {
            key,
            source: size,
            state: CacheState::Prepared,
        };
        let name = image.key.file_name();
        Self::reserve_cache(
            directory,
            name.as_str(),
            CACHE_HEADER_BYTES + pixels.len(),
            protected,
        )?;
        let file = directory
            .open_file_in_dir(name.as_str(), Mode::ReadWriteCreateOrTruncate)
            .map_err(AppDataError::Filesystem)?;
        file.write(&[0; CACHE_HEADER_BYTES])
            .map_err(AppDataError::Filesystem)?;
        file.write(pixels).map_err(AppDataError::Filesystem)?;
        file.flush().map_err(AppDataError::Filesystem)?;
        file.seek_from_start(0).map_err(AppDataError::Filesystem)?;
        file.write(&image.encode(pixels))
            .map_err(AppDataError::Filesystem)?;
        file.close().map_err(AppDataError::Filesystem)?;
        Self::read_cache(directory, &image.key, pixels)?.ok_or(AppDataError::ChecksumMismatch)?;
        Ok(image)
    }
}
