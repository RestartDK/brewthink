use super::{File, Result};
use brewthink::{
    app::ReaderPreferences,
    bounded_layout::{BoundedPage, ChapterSummary, MAX_ENCODED_PAGE_BYTES, layout_xhtml_stream},
    bounded_xml::stream::XmlWorkspace,
    chapter_cache::{
        CHAPTER_HEADER_BYTES, MAX_CHAPTER_BYTES, MAX_CHAPTER_CACHE_BYTES, MAX_CHAPTER_PAGES,
    },
    device_epub::resolve_resource_path,
    image::{PackedBitmap, PackedImage, READER_DEPTH, ScaleMode},
    image_cache::{ImageBytes, ImageSpec, ImageWorkspace},
    image_decoder::stream::MAX_IMAGE_FILE_BYTES,
    reader::render_inline_image,
    zip_stream::{ReadAt, StreamingZip, ZipValidationScratch},
};
use std::{convert::Infallible, fs, io, path::Path};

pub(super) struct Images<'a> {
    file: File<'a>,
    zip: Box<ZipValidationScratch>,
    workspace: Box<ImageWorkspace>,
    chapter: Option<Chapter>,
}

struct Chapter {
    path: String,
    text: Vec<u8>,
    preferences: ReaderPreferences,
    summary: ChapterSummary,
    records: Vec<Vec<u8>>,
}

struct Text<'a>(&'a [u8]);
impl ReadAt for Text<'_> {
    type Error = io::Error;
    fn len(&self) -> u32 {
        self.0.len() as u32
    }
    fn read_at(&self, offset: u32, output: &mut [u8]) -> io::Result<usize> {
        let start = (offset as usize).min(self.0.len());
        let count = output.len().min(self.0.len() - start);
        output[..count].copy_from_slice(&self.0[start..start + count]);
        Ok(count)
    }
}

impl<'a> Images<'a> {
    pub fn new(file: File<'a>) -> Self {
        Self {
            file,
            zip: Box::default(),
            workspace: Box::default(),
            chapter: None,
        }
    }

    pub fn layout(
        &mut self,
        path: &str,
        index: usize,
        preferences: ReaderPreferences,
    ) -> Result<BoundedPage> {
        if self
            .chapter
            .as_ref()
            .is_none_or(|chapter| chapter.path != path || chapter.preferences != preferences)
        {
            let text = match self.chapter.take() {
                Some(chapter) if chapter.path == path => chapter.text,
                _ => self.extract(path, MAX_CHAPTER_BYTES)?,
            };
            let archive = StreamingZip::open(self.file, &mut self.zip)
                .map_err(|error| format!("{error:?}"))?;
            let mut xml = Box::new(XmlWorkspace::new());
            let mut page = Box::new(BoundedPage::new());
            let mut record = Box::new([0; MAX_ENCODED_PAGE_BYTES]);
            let mut records = Vec::new();
            let mut file_bytes = CHAPTER_HEADER_BYTES + 4;
            let summary = layout_xhtml_stream(
                &Text(&text),
                &mut xml,
                preferences,
                &mut page,
                |href| {
                    Ok(resolve_resource_path::<Infallible>(path, href)
                        .ok()
                        .and_then(|path| {
                            self.workspace.probe_resource(&archive, path.as_str()).ok()
                        }))
                },
                |page| {
                    let length = page
                        .encode(&mut record[..])
                        .ok_or_else(|| io::Error::other("invalid page record"))?;
                    file_bytes += length + 12;
                    if records.len() == MAX_CHAPTER_PAGES
                        || file_bytes > MAX_CHAPTER_CACHE_BYTES as usize
                    {
                        return Err(io::Error::other("chapter exceeds page cache capacity"));
                    }
                    records.push(record[..length].to_vec());
                    Ok(())
                },
            )
            .map_err(|error| format!("{error:?}"))?;
            self.chapter = Some(Chapter {
                path: path.into(),
                text,
                preferences,
                summary,
                records,
            });
        }
        let chapter = self.chapter.as_ref().ok_or("chapter was not prepared")?;
        let bytes = chapter.records.get(index).ok_or("page is out of bounds")?;
        let mut page = BoundedPage::new();
        page.decode(bytes, index, chapter.summary, preferences)
            .ok_or("invalid cached page")?;
        Ok(page)
    }

    fn extract(&mut self, path: &str, limit: u32) -> Result<Vec<u8>> {
        let archive =
            StreamingZip::open(self.file, &mut self.zip).map_err(|error| format!("{error:?}"))?;
        let entry = archive.find(path).map_err(|error| format!("{error:?}"))?;
        if entry.uncompressed_size() > limit {
            return Err("resource exceeds the stream limit".into());
        }
        let mut encoded = Vec::new();
        archive
            .read_entry_to(entry, self.workspace.inflate(), &mut [0; 4096], |bytes| {
                encoded.extend_from_slice(bytes);
                Ok(())
            })
            .map_err(|error| format!("{error:?}"))?;
        Ok(encoded)
    }

    fn decode(&mut self, path: &str, spec: ImageSpec) -> Result<Vec<u8>> {
        let encoded = self.extract(path, MAX_IMAGE_FILE_BYTES)?;
        let mut pixels = vec![0xff; spec.byte_len()];
        self.workspace
            .decode(&ImageBytes(&encoded), spec, &mut pixels)
            .map_err(|error| format!("{error:?}"))?;
        Ok(pixels)
    }

    pub fn draw(&mut self, page: &BoundedPage, target: &mut PackedImage<'_>, offset: usize) {
        for image in page.images() {
            let pixels = self.decode(image.path(), image.spec()).ok();
            let bitmap = pixels.as_deref().and_then(|pixels| {
                PackedBitmap::new(image.spec().size(), READER_DEPTH, pixels).ok()
            });
            render_inline_image(image, bitmap, target, offset);
        }
    }

    pub fn covers(&mut self, path: Option<&str>, output: &Path) -> Result<Option<Vec<u8>>> {
        let mut statuses = String::new();
        let mut full_frame = None;
        let Some(path) = path else {
            statuses.push_str("cover: missing\n");
            fs::write(output.join("covers.txt"), statuses)?;
            return Ok(None);
        };
        let spec = ImageSpec::new(480, 800, ScaleMode::Contain).unwrap();
        match self.decode(path, spec) {
            Ok(bytes) => {
                statuses.push_str(&format!("cover: decoded {} bytes\n", bytes.len()));
                fs::write(output.join("cover.bin"), &bytes)?;
                full_frame = Some(bytes);
            }
            Err(error) => statuses.push_str(&format!("cover: {error}\n")),
        }
        fs::write(output.join("covers.txt"), statuses)?;
        Ok(full_frame)
    }
}
