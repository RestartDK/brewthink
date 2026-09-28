use super::*;
use crate::{
    bounded_layout::layout_xhtml_page_with_images_into,
    device_epub::resolve_resource_path,
    image_cache::{
        CACHE_HEADER_BYTES, CacheState, ImageKey, ImageResource, ImageSpec, ImageWorkspace,
        PreparedImage,
    },
    storage::{BookFile, BookFileName},
};
use core::cell::RefCell;
use std::{collections::VecDeque, rc::Rc};

pub(super) struct BookImages {
    bytes: Rc<[u8]>,
    file: BookFile,
    workspace: RefCell<Box<ImageWorkspace>>,
    zip: RefCell<Box<ZipValidationScratch>>,
    cache: RefCell<VecDeque<CachedPixels>>,
}

struct CachedPixels {
    image: PreparedImage,
    header: [u8; CACHE_HEADER_BYTES],
    pixels: Vec<u8>,
}

impl BookImages {
    pub fn new(bytes: &[u8], name: &str) -> Result<Self, SimulatorError> {
        let name = BookFileName::try_from(name).map_err(|_| SimulatorError::ArchiveTooLarge)?;
        let length = u32::try_from(bytes.len()).map_err(|_| SimulatorError::ArchiveTooLarge)?;
        Ok(Self {
            bytes: bytes.into(),
            file: BookFile::new(name, length),
            workspace: RefCell::new(Box::default()),
            zip: RefCell::new(Box::default()),
            cache: RefCell::new(VecDeque::new()),
        })
    }

    pub fn resource(&self, path: &str) -> Result<ImageResource, SimulatorError> {
        let archive = StreamingZip::open(
            MemoryFile::try_from(&self.bytes[..])?,
            &mut self.zip.borrow_mut(),
        )
        .map_err(SimulatorError::Zip)?;
        self.workspace
            .borrow_mut()
            .probe_resource(&archive, path)
            .map_err(SimulatorError::Image)
    }

    pub fn layout(
        &self,
        path: &str,
        xhtml: &[u8],
        index: usize,
        preferences: ReaderPreferences,
    ) -> Result<BoundedPage, LayoutError> {
        let reader =
            MemoryFile::try_from(&self.bytes[..]).expect("book address range checked at load");
        let archive = StreamingZip::open(reader, &mut self.zip.borrow_mut())
            .expect("book archive was validated at load");
        let mut workspace = self.workspace.borrow_mut();
        let mut page = BoundedPage::new();
        layout_xhtml_page_with_images_into(xhtml, index, preferences, &mut page, |href| {
            let resource = resolve_resource_path::<Infallible>(path, href).ok()?;
            workspace.probe_resource(&archive, resource.as_str()).ok()
        })?;
        Ok(page)
    }

    pub fn with_image<R>(
        &self,
        resource: &ImageResource,
        spec: ImageSpec,
        draw: impl FnOnce(PackedBitmap<'_>) -> R,
    ) -> Result<R, SimulatorError> {
        let key = ImageKey::resource(&self.file, resource, spec)
            .ok_or(SimulatorError::Image(ImageDecodeError::InvalidImage))?;
        let mut cache = self.cache.borrow_mut();
        let hit = cache
            .iter()
            .position(|entry| PreparedImage::decode(&entry.header, &key, &entry.pixels).is_some());
        let entry = if let Some(index) = hit {
            cache
                .remove(index)
                .expect("cache index came from the same deque")
        } else {
            let reader = MemoryFile::try_from(&self.bytes[..])?;
            let archive = StreamingZip::open(reader, &mut self.zip.borrow_mut())
                .map_err(SimulatorError::Zip)?;
            let entry = archive
                .find(resource.path.as_str())
                .map_err(SimulatorError::Zip)?;
            let mut staged = StagedImage {
                chunks: Vec::new(),
                length: 0,
            };
            let mut workspace = self.workspace.borrow_mut();
            archive
                .read_entry_to(entry, workspace.inflate(), &mut [0; 4096], |bytes| {
                    staged.chunks.push((staged.length, bytes.to_vec()));
                    staged.length += bytes.len() as u32;
                    Ok(())
                })
                .map_err(SimulatorError::Zip)?;
            let mut pixels = vec![0; spec.byte_len()];
            let source = workspace
                .decode(&staged, spec, &mut pixels)
                .map_err(SimulatorError::Image)?;
            let image = PreparedImage {
                key,
                source,
                state: CacheState::Prepared,
            };
            let header = image.encode(&pixels);
            CachedPixels {
                image,
                header,
                pixels,
            }
        };
        if cache.len() == 16 {
            cache.pop_front();
        }
        cache.push_back(entry);
        let entry = cache.back().expect("the prepared entry was just inserted");
        let bitmap = PackedBitmap::new(entry.image.key.spec().size(), READER_DEPTH, &entry.pixels)
            .expect("validated cache image");
        Ok(draw(bitmap))
    }
}

struct StagedImage {
    chunks: Vec<(u32, Vec<u8>)>,
    length: u32,
}

impl ReadAt for StagedImage {
    type Error = Infallible;
    fn len(&self) -> u32 {
        self.length
    }
    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Infallible> {
        if offset >= self.length {
            return Ok(0);
        }
        let index = self.chunks.partition_point(|(start, _)| *start <= offset) - 1;
        let (start, bytes) = &self.chunks[index];
        let bytes = &bytes[(offset - start) as usize..];
        let count = output.len().min(bytes.len());
        output[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }
}
