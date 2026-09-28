use std::{env, error::Error, fs, io};

use brewthink::{
    app::ReaderPreferences,
    bounded_layout::{BoundedPage, MAX_ENCODED_PAGE_BYTES, layout_xhtml_stream},
    bounded_xml::stream::XmlWorkspace,
    chapter_cache::{CHAPTER_HEADER_BYTES, MAX_CHAPTER_CACHE_BYTES, MAX_CHAPTER_PAGES},
    cover::{COVER_BYTES, COVER_HEIGHT, COVER_WIDTH},
    device_epub::{
        DeviceEpub, DevicePackageScratch, MAX_DEVICE_RESOURCE_BYTES, resolve_resource_path,
    },
    image::ScaleMode,
    image_cache::{ImageBytes, ImageSpec, ImageWorkspace},
    image_decoder::stream::MAX_IMAGE_FILE_BYTES,
    zip_stream::{InflateWorkspace, ReadAt, StreamingZip, ZipValidationScratch},
};

struct SliceFile<'a>(&'a [u8]);

impl ReadAt for SliceFile<'_> {
    type Error = io::Error;
    fn len(&self) -> u32 {
        self.0.len() as u32
    }
    fn read_at(&self, offset: u32, output: &mut [u8]) -> io::Result<usize> {
        let start = (offset as usize).min(self.0.len());
        let count = (self.0.len() - start).min(output.len());
        output[..count].copy_from_slice(&self.0[start..start + count]);
        Ok(count)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = env::args()
        .nth(1)
        .ok_or("usage: inspect-device-epub <book.epub>")?;
    let encoded = fs::read(path)?;
    u32::try_from(encoded.len()).map_err(|_| "EPUB exceeds archive address range")?;
    let mut zip = Box::new(ZipValidationScratch::new());
    let mut package = Box::new(DevicePackageScratch::new());
    let mut inflater = Box::new(InflateWorkspace::new());
    let mut resource = Box::new([0; MAX_DEVICE_RESOURCE_BYTES]);
    let mut publication = Box::new(brewthink::device_epub::DevicePublication::new());
    let book = DeviceEpub::open(
        SliceFile(&encoded),
        &mut zip,
        &mut package,
        &mut inflater,
        &mut resource,
        &mut publication,
    )
    .map_err(|error| format!("{error:?}"))?;
    let archive =
        StreamingZip::open(SliceFile(&encoded), &mut zip).map_err(|error| format!("{error:?}"))?;
    let mut images = Box::new(ImageWorkspace::new());
    println!("title: {}", book.publication().title());
    println!("creator: {}", book.publication().creator());
    println!("spine: {}", book.publication().spine_len());
    println!(
        "cover: {}",
        book.publication().cover_path().unwrap_or("none")
    );
    if let Some(path) = book.publication().cover_path() {
        let result = (|| -> Result<u32, Box<dyn Error>> {
            let entry = archive.find(path).map_err(|error| format!("{error:?}"))?;
            if entry.uncompressed_size() > MAX_IMAGE_FILE_BYTES {
                return Err("cover exceeds image stream limit".into());
            }
            let mut bytes = Vec::new();
            archive
                .read_entry_to(entry, images.inflate(), &mut resource[..4096], |chunk| {
                    bytes.extend_from_slice(chunk);
                    Ok(())
                })
                .map_err(|error| format!("{error:?}"))?;
            let mut packed = Box::new([0; COVER_BYTES]);
            let spec = ImageSpec::new(COVER_WIDTH, COVER_HEIGHT, ScaleMode::Cover).unwrap();
            images
                .decode(&ImageBytes(&bytes), spec, &mut packed[..])
                .map_err(|error| format!("{error:?}"))?;
            Ok(crc32fast::hash(&packed[..]))
        })();
        match result {
            Ok(crc) => println!("cover-packed-crc32: {crc:08x}"),
            Err(error) => println!("cover-error: {error}"),
        }
    }
    let preferences = ReaderPreferences::default();
    let mut xml = Box::new(XmlWorkspace::new());
    let mut page = Box::new(BoundedPage::new());
    let mut record = Box::new([0; MAX_ENCODED_PAGE_BYTES]);
    for index in 0..book.publication().spine_len() {
        let path = book.publication().spine_item(index).unwrap().path();
        let mut text = Vec::new();
        book.read_spine_to(index, &mut inflater, &mut resource[..4096], |chunk| {
            text.extend_from_slice(chunk);
            Ok(())
        })
        .map_err(|error| format!("spine {index} read: {error:?}"))?;
        let mut file_bytes = CHAPTER_HEADER_BYTES + 4;
        let mut last_length = 0;
        let summary = layout_xhtml_stream(
            &SliceFile(&text),
            &mut xml,
            preferences,
            &mut page,
            |href| {
                Ok(resolve_resource_path::<io::Error>(path, href)
                    .ok()
                    .and_then(|path| images.probe_resource(&archive, path.as_str()).ok()))
            },
            |page| {
                last_length = page
                    .encode(&mut record[..])
                    .ok_or_else(|| io::Error::other("invalid page record"))?;
                file_bytes += last_length + 12;
                if page.page_index() >= MAX_CHAPTER_PAGES
                    || file_bytes > MAX_CHAPTER_CACHE_BYTES as usize
                {
                    return Err(io::Error::other("chapter exceeds page cache capacity"));
                }
                Ok(())
            },
        )
        .map_err(|error| format!("spine {index} layout: {error:?}"))?;
        page.decode(
            &record[..last_length],
            summary.page_count - 1,
            summary,
            preferences,
        )
        .ok_or("invalid final page record")?;
        println!(
            "spine-{index}: {} bytes, {} pages, {}",
            text.len(),
            summary.page_count,
            summary.title.as_str()
        );
    }
    Ok(())
}
