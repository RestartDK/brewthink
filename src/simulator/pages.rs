use super::*;
use crate::{
    bounded_layout::{
        ChapterSummary, MAX_ENCODED_PAGE_BYTES, StreamLayoutError, layout_xhtml_stream,
    },
    bounded_xml::stream::XmlWorkspace,
    chapter_cache::{
        CHAPTER_HEADER_BYTES, MAX_CHAPTER_BYTES, MAX_CHAPTER_CACHE_BYTES, MAX_CHAPTER_PAGES,
    },
    image_cache::ImageResource,
};

pub(super) struct Pages {
    preferences: ReaderPreferences,
    summary: ChapterSummary,
    records: Vec<u8>,
    offsets: Vec<usize>,
}

impl Pages {
    pub fn build(
        text: &[u8],
        preferences: ReaderPreferences,
        mut image: impl FnMut(&str) -> Option<ImageResource>,
    ) -> Result<Self, LayoutError> {
        if text.len() > MAX_CHAPTER_BYTES as usize {
            return Err(LayoutError::ChapterCapacity);
        }
        let mut records = Vec::new();
        let mut offsets = vec![0];
        let mut xml = Box::new(XmlWorkspace::new());
        let mut page = Box::new(BoundedPage::new());
        let mut record = Box::new([0; MAX_ENCODED_PAGE_BYTES]);
        let summary = layout_xhtml_stream(
            &Text(text),
            &mut xml,
            preferences,
            &mut page,
            |href| Ok(image(href)),
            |page| {
                let length = page
                    .encode(&mut record[..])
                    .ok_or(LayoutError::InvalidPageRecord)?;
                let file_length = CHAPTER_HEADER_BYTES
                    + records.len()
                    + length
                    + offsets.len() * 8
                    + (offsets.len() + 1) * 4;
                if offsets.len() > MAX_CHAPTER_PAGES
                    || file_length > MAX_CHAPTER_CACHE_BYTES as usize
                {
                    return Err(LayoutError::ChapterCapacity);
                }
                records.extend_from_slice(&record[..length]);
                offsets.push(records.len());
                Ok(())
            },
        )
        .map_err(|error| match error {
            StreamLayoutError::Read(error)
            | StreamLayoutError::Write(error)
            | StreamLayoutError::Layout(error) => error,
        })?;
        Ok(Self {
            preferences,
            summary,
            records,
            offsets,
        })
    }

    pub fn preferences(&self) -> ReaderPreferences {
        self.preferences
    }

    pub fn page(&self, index: usize) -> Result<BoundedPage, LayoutError> {
        if index >= self.summary.page_count {
            return Err(LayoutError::PageOutOfBounds);
        }
        let mut page = BoundedPage::new();
        page.decode(
            &self.records[self.offsets[index]..self.offsets[index + 1]],
            index,
            self.summary,
            self.preferences,
        )
        .ok_or(LayoutError::InvalidPageRecord)?;
        Ok(page)
    }
}

struct Text<'a>(&'a [u8]);

impl ReadAt for Text<'_> {
    type Error = LayoutError;
    fn len(&self) -> u32 {
        self.0.len() as u32
    }
    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        let start = (offset as usize).min(self.0.len());
        let count = output.len().min(self.0.len() - start);
        output[..count].copy_from_slice(&self.0[start..start + count]);
        Ok(count)
    }
}
