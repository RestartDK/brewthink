use super::{File, Result};
use brewthink::{
    app::ReaderPreferences,
    bounded_layout::{BoundedPage, layout_xhtml_page_with_images_into},
    cover::{COVER_HEIGHT, COVER_WIDTH},
    device_epub::resolve_resource_path,
    image::{PackedBitmap, PackedImage, READER_DEPTH, ScaleMode},
    image_cache::{ImageBytes, ImageSpec, ImageWorkspace},
    image_decoder::stream::MAX_IMAGE_FILE_BYTES,
    reader::render_inline_image,
    zip_stream::{StreamingZip, ZipValidationScratch},
};
use std::{convert::Infallible, fs, path::Path};

pub(super) struct Images<'a> {
    file: File<'a>,
    zip: Box<ZipValidationScratch>,
    workspace: Box<ImageWorkspace>,
}

impl<'a> Images<'a> {
    pub fn new(file: File<'a>) -> Self {
        Self {
            file,
            zip: Box::default(),
            workspace: Box::default(),
        }
    }

    pub fn layout(
        &mut self,
        path: &str,
        xhtml: &[u8],
        index: usize,
        preferences: ReaderPreferences,
    ) -> Result<BoundedPage> {
        let archive =
            StreamingZip::open(self.file, &mut self.zip).map_err(|error| format!("{error:?}"))?;
        let mut page = BoundedPage::new();
        layout_xhtml_page_with_images_into(xhtml, index, preferences, &mut page, |href| {
            let path = resolve_resource_path::<Infallible>(path, href).ok()?;
            self.workspace.probe_resource(&archive, path.as_str()).ok()
        })?;
        Ok(page)
    }

    fn decode(&mut self, path: &str, spec: ImageSpec) -> Result<Vec<u8>> {
        let archive =
            StreamingZip::open(self.file, &mut self.zip).map_err(|error| format!("{error:?}"))?;
        let entry = archive.find(path).map_err(|error| format!("{error:?}"))?;
        if entry.uncompressed_size() > MAX_IMAGE_FILE_BYTES {
            return Err("encoded image exceeds the stream limit".into());
        }
        let mut encoded = Vec::new();
        archive
            .read_entry_to(entry, self.workspace.inflate(), &mut [0; 4096], |bytes| {
                encoded.extend_from_slice(bytes);
                Ok(())
            })
            .map_err(|error| format!("{error:?}"))?;
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
        for (name, spec) in [
            (
                "shelf",
                ImageSpec::new(COVER_WIDTH, COVER_HEIGHT, ScaleMode::Cover).unwrap(),
            ),
            (
                "cover",
                ImageSpec::new(480, 800, ScaleMode::Contain).unwrap(),
            ),
        ] {
            let Some(path) = path else {
                statuses.push_str(&format!("{name}: missing\n"));
                continue;
            };
            match self.decode(path, spec) {
                Ok(bytes) => {
                    statuses.push_str(&format!("{name}: decoded {} bytes\n", bytes.len()));
                    fs::write(output.join(format!("{name}.bin")), &bytes)?;
                    if name == "cover" {
                        full_frame = Some(bytes);
                    }
                }
                Err(error) => statuses.push_str(&format!("{name}: {error}\n")),
            }
        }
        fs::write(output.join("covers.txt"), statuses)?;
        Ok(full_frame)
    }
}
