use super::*;
use crate::{
    app::{ReaderFont, ReaderFontSize, ReaderSpacing},
    bounded_xml::XmlError,
};

const EPUB: &[u8] = include_bytes!("../../web/tests/fixtures/minimal.epub");

#[test]
fn metadata_chapters_and_pages_match_the_device_pipeline() {
    let imported = Book::from_epub(EPUB, "minimal.epub").unwrap();
    let mut zip = Box::new(ZipValidationScratch::new());
    let mut package = Box::new(DevicePackageScratch::new());
    let mut inflater = Box::new(InflateWorkspace::new());
    let mut resource = Box::new([0; MAX_DEVICE_RESOURCE_BYTES]);
    let device = DeviceEpub::open(
        MemoryFile::try_from(EPUB).unwrap(),
        &mut zip,
        &mut package,
        &mut inflater,
        &mut resource,
    )
    .unwrap();
    assert_eq!(imported.title, device.publication().title());
    assert_eq!(imported.creator, device.publication().creator());
    assert_eq!(imported.chapters.len(), device.publication().spine_len());
    for (index, chapter) in imported.chapters.iter().enumerate() {
        let length = device
            .read_spine(index, &mut resource[..], &mut inflater)
            .unwrap();
        for font in [ReaderFont::NotoSerif, ReaderFont::Compact, ReaderFont::Mono] {
            for size in [
                ReaderFontSize::Small,
                ReaderFontSize::Medium,
                ReaderFontSize::Large,
            ] {
                for spacing in [
                    ReaderSpacing::Compact,
                    ReaderSpacing::Normal,
                    ReaderSpacing::Relaxed,
                ] {
                    let preferences = ReaderPreferences::new(font, size, spacing);
                    let first = layout_xhtml_page(&resource[..length], 0, preferences).unwrap();
                    for page_index in 0..first.page_count() {
                        let actual = chapter.page(page_index, preferences).unwrap();
                        let expected =
                            layout_xhtml_page(&resource[..length], page_index, preferences)
                                .unwrap();
                        assert_eq!(actual.page_count(), expected.page_count());
                        assert_eq!(actual.chapter_title(), expected.chapter_title());
                        assert_eq!(
                            actual
                                .lines()
                                .map(|line| (line.text(), line.style()))
                                .collect::<Vec<_>>(),
                            expected
                                .lines()
                                .map(|line| (line.text(), line.style()))
                                .collect::<Vec<_>>()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn chapter_resource_limit_and_malformed_xml_are_not_hidden() {
    let oversized = vec![b' '; MAX_DEVICE_RESOURCE_BYTES + 1];
    assert!(matches!(
        Chapter::from_xhtml(&oversized),
        Err(SimulatorError::Epub(DeviceEpubError::ResourceTooLarge))
    ));
    assert!(matches!(
        Chapter::from_xhtml(b"<body><p>text</bad></body>"),
        Err(SimulatorError::Layout(LayoutError::Xml(
            XmlError::Malformed
        )))
    ));
}

#[test]
fn png_cover_bytes_match_the_device_decoder() {
    let book = Book::from_epub(EPUB, "minimal.epub").unwrap();
    let mut zip = ZipValidationScratch::new();
    let archive = StreamingZip::open(MemoryFile::try_from(EPUB).unwrap(), &mut zip).unwrap();
    let entry = archive.find("EPUB/cover.png").unwrap();
    let mut inflater = Box::new(InflateWorkspace::new());
    let mut resource = Box::new([0; MAX_DEVICE_RESOURCE_BYTES]);
    let length = archive
        .read_entry(entry, &mut resource[..], &mut inflater)
        .unwrap();
    let mut expected = [0xff; COVER_BYTES];
    cover::decode_png_cover(
        &resource[..length],
        &mut expected,
        &mut CoverDecodeWorkspace::new(),
    )
    .unwrap();
    let Cover::Decoded(actual) = book.cover else {
        panic!("decoded cover expected");
    };
    assert_eq!(actual.as_ref(), &expected);
    assert!(actual.iter().any(|byte| *byte != 0xff));
}

#[test]
fn sample_chapters_use_the_bounded_pipeline_after_reflow() {
    for book in sample_books().unwrap() {
        for chapter in book.chapters {
            let page = chapter.page(0, ReaderPreferences::default()).unwrap();
            assert!(page.page_count() > 1);
            assert!(page.chapter_title().contains(&book.title));
            assert!(matches!(
                chapter.page(page.page_count(), ReaderPreferences::default()),
                Err(LayoutError::PageOutOfBounds)
            ));
        }
    }
}
