extern crate std;

use super::*;
use crate::{
    app::{ReaderFont, ReaderFontSize, ReaderSpacing},
    bounded_layout::{BoundedPage, layout_xhtml_page},
    image_cache::ImageBytes,
    storage::BookFileName,
    zip_stream::{InflateWorkspace, StreamingZip, ZipValidationScratch},
};
use std::{boxed::Box, vec};

fn key() -> ChapterKey {
    let bytes = include_bytes!("../../web/tests/fixtures/minimal.epub");
    let book = BookFile::new(
        BookFileName::try_from("BOOK.EPB").unwrap(),
        bytes.len() as u32,
    );
    let mut scratch = Box::new(ZipValidationScratch::new());
    let archive = StreamingZip::open(ImageBytes(bytes), &mut scratch).unwrap();
    ChapterKey::new(
        ChapterSourceKey::new(
            &book,
            archive.find("EPUB/chapter.xhtml").unwrap(),
            archive.directory_crc32().unwrap(),
        )
        .unwrap(),
        ReaderPreferences::default(),
    )
}

#[test]
fn staged_text_checksum_requires_complete_sequential_reads_and_detects_changed_bytes() {
    let key = key();
    let mut staged = key.source.header().to_vec();
    let mut zip = Box::new(ZipValidationScratch::new());
    let archive = StreamingZip::open(
        ImageBytes(include_bytes!("../../web/tests/fixtures/minimal.epub")),
        &mut zip,
    )
    .unwrap();
    archive
        .read_entry_to(
            archive.find(key.source.path()).unwrap(),
            &mut Box::new(InflateWorkspace::new()),
            &mut [0; 128],
            |bytes| {
                staged.extend_from_slice(bytes);
                Ok(())
            },
        )
        .unwrap();
    for damaged in [false, true] {
        if damaged {
            staged[SOURCE_HEADER_BYTES] ^= 1;
        }
        let source = ImageBytes(&staged);
        let text = ChapterText::new(&source, key.source()).unwrap();
        assert_eq!(text.checksum(), None);
        let mut offset = 0;
        let mut buffer = [0; 19];
        while offset < text.len() {
            offset += text.read_at(offset, &mut buffer).unwrap() as u32;
        }
        assert_eq!(text.checksum() == Some(key.source.checksum()), !damaged);
        let nonsequential = ChapterText::new(&source, key.source()).unwrap();
        nonsequential.read_at(1, &mut buffer).unwrap();
        let mut output = vec![0; text.len() as usize];
        nonsequential.read_at(0, &mut output).unwrap();
        assert_eq!(nonsequential.checksum(), None);
    }
}

#[test]
fn chapter_workspace_reuses_the_existing_resource_allocation() {
    assert!(core::mem::size_of::<LayoutWorkspace>() <= MAX_DEVICE_RESOURCE_BYTES);
    assert_eq!(
        core::mem::size_of::<ChapterWorkspace>(),
        MAX_DEVICE_RESOURCE_BYTES + 8
    );
    let mut workspace = Box::new(ChapterWorkspace::new());
    workspace.bytes().fill(0xa5);
    let layout = workspace.layout();
    assert!(layout.index.iter().all(|&byte| byte == 0));
    assert!(layout.record.iter().all(|&byte| byte == 0));
    assert!(workspace.bytes().iter().all(|&byte| byte == 0));
}

#[test]
fn headers_reject_corruption_wrong_source_typography_and_invalid_index_ranges() {
    let key = key();
    let mut index = [0; 12];
    for (target, value) in index.chunks_exact_mut(4).zip([540u32, 560, 580]) {
        target.copy_from_slice(&value.to_le_bytes());
    }
    let header = ChapterHeader {
        summary: ChapterSummary {
            page_count: 2,
            title: FixedString::try_from_str("Chapter α").unwrap(),
        },
        index_offset: 580,
        file_length: 592,
        index_crc: crc32fast::hash(&index),
    };
    let bytes = header.encode(&key);
    assert_eq!(ChapterHeader::decode(&bytes, &key, 592), Some(header));
    assert_eq!(header.page_range(&index, 1), Some(560..580));
    for byte in 0..bytes.len() {
        let mut corrupt = bytes;
        corrupt[byte] ^= 1;
        assert!(
            ChapterHeader::decode(&corrupt, &key, 592).is_none(),
            "byte={byte}"
        );
    }
    let preferences = ReaderPreferences::new(
        ReaderFont::Mono,
        ReaderFontSize::Large,
        ReaderSpacing::Relaxed,
    );
    let other = ChapterKey::new(key.source.clone(), preferences);
    assert!(ChapterHeader::decode(&bytes, &other, 592).is_none());
    let mut other = key.clone();
    other.source.bytes[399] ^= 1;
    assert!(ChapterHeader::decode(&bytes, &other, 592).is_none());
    assert!(ChapterHeader::decode(&bytes, &key, 593).is_none());
    for (offset, value) in [
        (416, 0u32),
        (416, MAX_CHAPTER_PAGES as u32 + 1),
        (420, 539),
        (420, u32::MAX),
    ] {
        let mut corrupt = bytes;
        corrupt[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let crc = crc32fast::hash(&corrupt[..532]);
        corrupt[532..536].copy_from_slice(&crc.to_le_bytes());
        assert!(ChapterHeader::decode(&corrupt, &key, 592).is_none());
    }
    index[4..8].copy_from_slice(&540u32.to_le_bytes());
    let header = ChapterHeader {
        index_crc: crc32fast::hash(&index),
        ..header
    };
    assert!(header.page_range(&index, 0).is_none());
    let source_header = key.source.header();
    assert!(key.source.matches_header(
        &source_header,
        SOURCE_HEADER_BYTES as u32 + key.source.length()
    ));
    for byte in 0..source_header.len() {
        let mut corrupt = source_header;
        corrupt[byte] ^= 1;
        assert!(
            !key.source
                .matches_header(&corrupt, SOURCE_HEADER_BYTES as u32 + key.source.length())
        );
    }
}

#[test]
fn page_records_preserve_geometry_and_reject_truncation_and_unknown_styles() {
    let preferences = ReaderPreferences::default();
    let original = layout_xhtml_page(
        b"<html><body><h1>Title</h1><p>Body &amp; text</p><pre>  indented</pre></body></html>",
        0,
        preferences,
    )
    .unwrap();
    let mut encoded = vec![0; MAX_ENCODED_PAGE_BYTES];
    let length = original.encode(&mut encoded).unwrap();
    encoded.truncate(length);
    let summary = ChapterSummary {
        page_count: original.page_count(),
        title: FixedString::try_from_str(original.chapter_title()).unwrap(),
    };
    let mut page = Box::new(BoundedPage::new());
    page.decode(&encoded, 0, summary, preferences).unwrap();
    assert_eq!(*page, original);
    for length in 0..encoded.len() {
        assert!(
            page.decode(&encoded[..length], 0, summary, preferences)
                .is_none()
        );
    }
    encoded[4] = 255;
    assert!(page.decode(&encoded, 0, summary, preferences).is_none());
    assert!(page.decode(&encoded, 1, summary, preferences).is_none());
    assert!(page.decode(&[0], 0, summary, preferences).is_none());
    assert!(page.decode(&[51], 0, summary, preferences).is_none());
}
