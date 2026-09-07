use super::*;
use crate::{
    app::{ReaderFont, ReaderFontSize, ReaderSpacing},
    bounded_xml::XmlError,
};

const EPUB: &[u8] = include_bytes!("../../web/tests/fixtures/minimal.epub");
const TEXT: &[u8] = include_bytes!("../../web/tests/fixtures/parity/text.epub");
const JPEG: &[u8] = include_bytes!("../../web/tests/fixtures/parity/jpeg.epub");
const NCX: &[u8] = include_bytes!("../../web/tests/fixtures/parity/ncx.epub");
const NO_NAV: &[u8] = include_bytes!("../../web/tests/fixtures/parity/no-nav.epub");
const MALFORMED_NAV: &[u8] = include_bytes!("../../web/tests/fixtures/parity/malformed-nav.epub");
const NO_COVER: &[u8] = include_bytes!("../../web/tests/fixtures/parity/no-cover.epub");
const UNSUPPORTED_COVER: &[u8] =
    include_bytes!("../../web/tests/fixtures/parity/unsupported-cover.epub");
const BROKEN_COVER: &[u8] = include_bytes!("../../web/tests/fixtures/parity/broken-cover.epub");
const BROKEN_JPEG: &[u8] = include_bytes!("../../web/tests/fixtures/parity/broken-jpeg.epub");
const FRAME_LIMIT: &[u8] = include_bytes!("../../web/tests/fixtures/parity/frame-limit.epub");
const SHELF_ONLY_COVER: &[u8] =
    include_bytes!("../../web/tests/fixtures/parity/shelf-only-cover.epub");
const SHELF_LIMIT: &[u8] = include_bytes!("../../web/tests/fixtures/parity/shelf-limit.epub");
const OVERSIZED_COVER: &[u8] =
    include_bytes!("../../web/tests/fixtures/parity/oversized-cover.epub");
const COMPRESSED_OVERSIZED_COVER: &[u8] =
    include_bytes!("../../web/tests/fixtures/parity/compressed-oversized-cover.epub");

const PARITY_COVER_PATH: &str = "OPS/cover";

struct ZipEntryBytes {
    compressed: u32,
    uncompressed: u32,
    bytes: Vec<u8>,
}

fn zip_entry(epub: &[u8], path: &str) -> ZipEntryBytes {
    let mut zip = Box::new(ZipValidationScratch::new());
    let archive = StreamingZip::open(MemoryFile::try_from(epub).unwrap(), &mut zip).unwrap();
    let entry = archive.find(path).unwrap();
    let mut inflater = Box::new(InflateWorkspace::new());
    let mut resource = Box::new([0; MAX_DEVICE_RESOURCE_BYTES]);
    let length = archive
        .read_entry(entry, &mut resource[..], &mut inflater)
        .unwrap();
    ZipEntryBytes {
        compressed: entry.compressed_size(),
        uncompressed: entry.uncompressed_size(),
        bytes: resource[..length].to_vec(),
    }
}

fn native_shelf(encoded: &[u8]) -> Result<Box<[u8; COVER_BYTES]>, ImageDecodeError> {
    let mut output = Box::new([0xff; COVER_BYTES]);
    match ImageFormat::detect(encoded).unwrap() {
        ImageFormat::Png => {
            cover::decode_png_cover(encoded, &mut output, &mut CoverDecodeWorkspace::new())?
        }
        ImageFormat::Jpeg => {
            cover::decode_jpeg_cover(encoded, &mut output, &mut JpegDecodeWorkspace::new())?
        }
    }
    Ok(output)
}

fn native_frame(encoded: &[u8]) -> Result<Box<[u8; FRAME_BYTES]>, ImageDecodeError> {
    let mut output = Box::new([0xff; FRAME_BYTES]);
    let mut target = PackedImage::new(frame_size(), READER_DEPTH, &mut output[..]).unwrap();
    let options = RenderOptions {
        scale: ScaleMode::Contain,
        dither: Dither::None,
    };
    match ImageFormat::detect(encoded).unwrap() {
        ImageFormat::Png => decode_png(
            encoded,
            &mut target,
            options,
            &mut CoverDecodeWorkspace::new(),
        )?,
        ImageFormat::Jpeg => decode_jpeg(
            encoded,
            &mut target,
            options,
            &mut JpegDecodeWorkspace::new(),
        )?,
    };
    Ok(output)
}

fn chapter_titles(book: &Book) -> Vec<&str> {
    book.chapters.iter().map(Chapter::title).collect()
}

fn decoded_parts(cover: &Cover) -> (&[u8; COVER_BYTES], &OriginalFrame) {
    let Cover::Decoded { shelf, original } = cover else {
        panic!("decoded cover expected");
    };
    (shelf, original)
}

#[test]
fn metadata_chapters_and_pages_match_the_device_pipeline() {
    let imported = Book::from_epub(EPUB, "minimal.epub").unwrap();
    let mut zip = Box::new(ZipValidationScratch::new());
    let mut package = Box::new(DevicePackageScratch::new());
    let mut inflater = Box::new(InflateWorkspace::new());
    let mut resource = Box::new([0; MAX_DEVICE_RESOURCE_BYTES]);
    let mut publication = Box::new(DevicePublication::new());
    let device = DeviceEpub::open(
        MemoryFile::try_from(EPUB).unwrap(),
        &mut zip,
        &mut package,
        &mut inflater,
        &mut resource,
        &mut publication,
    )
    .unwrap();
    let mut titles = Box::new([FixedString::<CHAPTER_TITLE_BYTES>::new(); MAX_DEVICE_SPINE_ITEMS]);
    device
        .read_chapter_titles(&mut titles, &mut resource[..], &mut inflater)
        .unwrap();
    assert!(imported.navigation_error.is_none());
    assert_eq!(imported.title, device.publication().title());
    assert_eq!(imported.creator, device.publication().creator());
    assert_eq!(imported.chapters.len(), device.publication().spine_len());
    for (index, chapter) in imported.chapters.iter().enumerate() {
        let expected_title = match titles[index].as_str() {
            "" => format!("Chapter {}", index + 1),
            title => title.into(),
        };
        assert_eq!(chapter.title(), expected_title);
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
        Chapter::from_xhtml(&oversized, "Chapter 1".into()),
        Err(SimulatorError::Epub(DeviceEpubError::ResourceTooLarge))
    ));
    assert!(matches!(
        Chapter::from_xhtml(b"<body><p>text</bad></body>", "Chapter 1".into()),
        Err(SimulatorError::Layout(LayoutError::Xml(
            XmlError::Malformed
        )))
    ));
}

#[test]
fn epub3_nav_and_ncx_titles_name_every_spine_item() {
    for (name, epub) in [("text.epub", TEXT), ("ncx.epub", NCX)] {
        let book = Book::from_epub(epub, name).unwrap();
        assert_eq!(chapter_titles(&book), ["Opening", "Closing"], "{name}");
        assert!(book.navigation_error.is_none(), "{name}");
    }
}

#[test]
fn missing_navigation_numbers_chapters_without_an_error() {
    let book = Book::from_epub(NO_NAV, "no-nav.epub").unwrap();
    assert_eq!(chapter_titles(&book), ["Chapter 1", "Chapter 2"]);
    assert!(book.navigation_error.is_none());
}

#[test]
fn malformed_navigation_clears_partial_titles_and_keeps_the_cause() {
    let book = Book::from_epub(MALFORMED_NAV, "malformed-nav.epub").unwrap();
    assert_eq!(chapter_titles(&book), ["Chapter 1", "Chapter 2"]);
    assert_eq!(
        book.navigation_error,
        Some(DeviceEpubError::Xml(XmlError::Malformed))
    );
    for chapter in &book.chapters {
        assert!(
            chapter
                .page(0, ReaderPreferences::default())
                .unwrap()
                .page_count()
                >= 1
        );
    }
}

#[test]
fn png_cover_bytes_match_the_device_decoder() {
    let book = Book::from_epub(EPUB, "minimal.epub").unwrap();
    let encoded = zip_entry(EPUB, "EPUB/cover.png").bytes;
    let (shelf, original) = decoded_parts(&book.cover);
    assert_eq!(shelf, &*native_shelf(&encoded).unwrap());
    assert!(shelf.iter().any(|byte| *byte != 0xff));
    let OriginalFrame::Decoded(frame) = original else {
        panic!("decoded original frame expected");
    };
    assert_eq!(frame, &native_frame(&encoded).unwrap());
    assert!(frame.iter().any(|byte| *byte != 0xff));
}

#[test]
fn png_and_jpeg_frames_match_the_native_contain_decode() {
    for (name, epub) in [("text.epub", TEXT), ("jpeg.epub", JPEG)] {
        let book = Book::from_epub(epub, name).unwrap();
        let encoded = zip_entry(epub, PARITY_COVER_PATH).bytes;
        let (shelf, original) = decoded_parts(&book.cover);
        assert_eq!(shelf, &*native_shelf(&encoded).unwrap(), "{name}");
        let OriginalFrame::Decoded(frame) = original else {
            panic!("{name}: decoded original frame expected");
        };
        assert_eq!(frame, &native_frame(&encoded).unwrap(), "{name}");
        assert_eq!(frame.len(), 96_000);
        let bitmap = book.cover.frame_bitmap().unwrap();
        assert_eq!(bitmap.size(), Size::new(480, 800).unwrap());
        assert_eq!(bitmap.depth(), READER_DEPTH);
        assert_eq!(
            book.cover.bitmap().unwrap().size(),
            Size::new(176, 264).unwrap()
        );
    }
}

#[test]
fn frame_gate_is_the_uncompressed_size_and_shelf_gate_is_128_kib() {
    let cases = [
        ("frame-limit.epub", FRAME_LIMIT, 98_304, true, true),
        (
            "shelf-only-cover.epub",
            SHELF_ONLY_COVER,
            98_305,
            true,
            false,
        ),
        ("shelf-limit.epub", SHELF_LIMIT, 131_072, true, false),
        (
            "oversized-cover.epub",
            OVERSIZED_COVER,
            131_073,
            false,
            false,
        ),
    ];
    for (name, epub, size, shelf_expected, frame_expected) in cases {
        let entry = zip_entry(epub, PARITY_COVER_PATH);
        assert_eq!(entry.uncompressed, size, "{name}");
        assert_eq!(entry.compressed, size, "{name}");
        let book = Book::from_epub(epub, name).unwrap();
        assert_eq!(book.cover.bitmap().is_some(), shelf_expected, "{name}");
        assert_eq!(
            book.cover.frame_bitmap().is_some(),
            frame_expected,
            "{name}"
        );
        match (&book.cover, shelf_expected, frame_expected) {
            (Cover::Decoded { shelf, original }, true, true) => {
                assert_eq!(shelf, &native_shelf(&entry.bytes).unwrap(), "{name}");
                let OriginalFrame::Decoded(frame) = original else {
                    panic!("{name}: decoded original frame expected");
                };
                assert_eq!(frame, &native_frame(&entry.bytes).unwrap(), "{name}");
            }
            (Cover::Decoded { shelf, original }, true, false) => {
                assert_eq!(shelf, &native_shelf(&entry.bytes).unwrap(), "{name}");
                assert!(matches!(original, OriginalFrame::TooLarge), "{name}");
                assert!(
                    native_frame(&entry.bytes).is_ok(),
                    "{name}: only the gate rejects it"
                );
            }
            (Cover::TooLarge, false, false) => {}
            _ => panic!("{name}: unexpected cover outcome"),
        }
    }
}

#[test]
fn compressed_size_above_128_kib_rejects_the_cover_before_reading() {
    let entry = zip_entry(COMPRESSED_OVERSIZED_COVER, PARITY_COVER_PATH);
    assert_eq!(entry.compressed, 131_225);
    assert_eq!(entry.uncompressed, 145);
    assert!(native_frame(&entry.bytes).is_ok());
    let book = Book::from_epub(
        COMPRESSED_OVERSIZED_COVER,
        "compressed-oversized-cover.epub",
    )
    .unwrap();
    assert!(matches!(book.cover, Cover::TooLarge));
    assert!(book.cover.bitmap().is_none());
    assert!(book.cover.frame_bitmap().is_none());
}

#[test]
fn absent_unsupported_and_broken_covers_stay_distinct() {
    let missing = Book::from_epub(NO_COVER, "no-cover.epub").unwrap();
    assert!(matches!(missing.cover, Cover::Missing));
    let unsupported = Book::from_epub(UNSUPPORTED_COVER, "unsupported-cover.epub").unwrap();
    assert!(matches!(unsupported.cover, Cover::Unsupported));
    for (name, epub) in [
        ("broken-cover.epub", BROKEN_COVER),
        ("broken-jpeg.epub", BROKEN_JPEG),
    ] {
        let book = Book::from_epub(epub, name).unwrap();
        assert!(
            matches!(
                book.cover,
                Cover::Failed(SimulatorError::Image(ImageDecodeError::InvalidImage))
            ),
            "{name}"
        );
        assert_eq!(chapter_titles(&book), ["Opening", "Closing"], "{name}");
    }
    for book in [&missing, &unsupported] {
        assert!(book.cover.bitmap().is_none());
        assert!(book.cover.frame_bitmap().is_none());
    }
}

#[test]
fn truncated_sources_fail_the_shelf_and_frame_decoders_alike() {
    for (name, epub) in [
        ("broken-cover.epub", BROKEN_COVER),
        ("broken-jpeg.epub", BROKEN_JPEG),
    ] {
        let encoded = zip_entry(epub, PARITY_COVER_PATH).bytes;
        assert_eq!(
            native_shelf(&encoded).unwrap_err(),
            ImageDecodeError::InvalidImage,
            "{name}"
        );
        assert_eq!(
            native_frame(&encoded).unwrap_err(),
            ImageDecodeError::InvalidImage,
            "{name}"
        );
    }
}

#[test]
fn sample_chapters_use_the_bounded_pipeline_after_reflow() {
    for book in sample_books().unwrap() {
        assert!(book.navigation_error.is_none());
        for (index, chapter) in book.chapters.iter().enumerate() {
            assert_eq!(chapter.title(), format!("Section {}", index + 1));
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

#[test]
fn sample_covers_render_the_shelf_and_the_original_frame() {
    for book in sample_books().unwrap() {
        let shelf = book.cover.bitmap().unwrap();
        let frame = book.cover.frame_bitmap().unwrap();
        assert_eq!(shelf.size(), Size::new(176, 264).unwrap());
        assert_eq!(frame.size(), Size::new(480, 800).unwrap());
        let (shelf_bytes, original) = decoded_parts(&book.cover);
        let OriginalFrame::Decoded(frame_bytes) = original else {
            panic!("sample frame expected");
        };
        assert!(shelf_bytes.iter().any(|byte| *byte != 0xff));
        assert!(frame_bytes.iter().any(|byte| *byte != 0xff));
        assert_eq!(frame.luma(0, 0), 255, "contain leaves the frame edge white");
    }
}
