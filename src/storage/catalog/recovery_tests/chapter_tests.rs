use super::*;
use crate::{
    app::{ReaderFont, ReaderFontSize, ReaderPreferences, ReaderSpacing},
    bounded_layout::BoundedPage,
    chapter_cache::{
        CHAPTER_HEADER_BYTES, ChapterRequest, ChapterWorkspace, PreparedChapter,
        SOURCE_HEADER_BYTES,
    },
    image_cache::{CacheState, ImageWorkspace},
    transfer::BookName,
    zip_stream::ZipValidationScratch,
};
use std::{boxed::Box, format};

const MINIMAL: &[u8] = include_bytes!("../../../../web/tests/fixtures/minimal.epub");
const LARGE: &[u8] = include_bytes!("../../../../web/tests/fixtures/streamed-chapters.epub");

fn fixture(bytes: &[u8]) -> (Card, BookFile) {
    let card = Card::formatted();
    let store = card.store();
    let data = store.app_data();
    let request = UploadRequest::book(
        BookName::parse("BOOK.EPB").unwrap(),
        bytes.len(),
        crc32fast::hash(bytes),
    );
    data.begin_upload(request).unwrap();
    for chunk in bytes.chunks(4096) {
        data.append_upload(chunk).unwrap();
    }
    data.commit_upload(request, &mut [0; 4096]).unwrap();
    let catalog = store.scan::<16>().unwrap();
    (card, *catalog.books().next().unwrap())
}

struct Harness {
    workspace: Box<ChapterWorkspace>,
    images: Box<ImageWorkspace>,
    zip: Box<ZipValidationScratch>,
    page: Box<BoundedPage>,
}

impl Harness {
    fn new() -> Self {
        Self {
            workspace: Box::new(ChapterWorkspace::new()),
            images: Box::new(ImageWorkspace::new()),
            zip: Box::new(ZipValidationScratch::new()),
            page: Box::new(BoundedPage::new()),
        }
    }
    fn load(
        &mut self,
        card: &Card,
        book: &BookFile,
        path: &str,
        page_index: usize,
        preferences: ReaderPreferences,
    ) -> Result<PreparedChapter, AppDataError<Fault>> {
        card.store().app_data().chapter_page(
            ChapterRequest {
                book,
                path,
                page_index,
                preferences,
            },
            &mut self.workspace,
            &mut self.images,
            &mut self.zip,
            &mut self.page,
        )
    }
}

fn original_unchanged(card: &Card, book: &BookFile, original: &[u8]) {
    let mut readback = vec![0; original.len()];
    assert_eq!(
        card.store().read_at(book.name(), 0, &mut readback).unwrap(),
        original.len()
    );
    assert_eq!(readback, original);
}

fn corrupt(card: &Card, name: &str, offset: u32) {
    card.store()
        .with_app_directory(|app| {
            let cache = app.open_dir(CACHE_DIRECTORY)?;
            let file = cache.open_file_in_dir(name, Mode::ReadWriteAppend)?;
            file.seek_from_start(offset)?;
            let mut byte = [0];
            file.read(&mut byte)?;
            byte[0] ^= 0xff;
            file.seek_from_start(offset)?;
            file.write(&byte)?;
            file.close()?;
            cache.close()
        })
        .unwrap();
}

#[test]
fn oversized_chapters_cache_every_page_across_remount_and_reflow_with_images() {
    let (card, book) = fixture(LARGE);
    let mut harness = Harness::new();
    let preferences = ReaderPreferences::default();
    let first = harness
        .load(&card, &book, "EPUB/long.xhtml", 0, preferences)
        .unwrap();
    assert_eq!(first.state, CacheState::Prepared);
    assert!(first.summary.page_count > 1000);
    assert_eq!(harness.page.images().count(), 1);
    let opening = *harness.page;
    let before_hits = card.0.borrow().bytes.clone();
    let last = first.summary.page_count - 1;
    for index in [last, last / 2, 1, 0] {
        let loaded = harness
            .load(&card, &book, "EPUB/long.xhtml", index, preferences)
            .unwrap();
        assert_eq!(loaded.state, CacheState::Hit);
        assert_eq!(loaded.summary, first.summary);
        assert_eq!(harness.page.page_index(), index);
        if index == last {
            assert!(
                harness
                    .page
                    .lines()
                    .any(|line| line.text().contains("FINAL CHAPTER"))
            );
        }
    }
    assert_eq!(*harness.page, opening);
    assert_eq!(
        card.0.borrow().bytes,
        before_hits,
        "warm hits preserve every stored byte"
    );
    let preferences = ReaderPreferences::new(
        ReaderFont::Mono,
        ReaderFontSize::Large,
        ReaderSpacing::Relaxed,
    );
    let reflow = harness
        .load(&card, &book, "EPUB/long.xhtml", 0, preferences)
        .unwrap();
    assert_eq!(reflow.state, CacheState::Prepared);
    assert_ne!(reflow.key, first.key);
    assert_ne!(reflow.summary.page_count, first.summary.page_count);
    let paragraph = harness
        .load(&card, &book, "EPUB/paragraph.xhtml", 0, preferences)
        .unwrap();
    harness
        .load(
            &card,
            &book,
            "EPUB/paragraph.xhtml",
            paragraph.summary.page_count - 1,
            preferences,
        )
        .unwrap();
    assert!(
        harness
            .page
            .lines()
            .any(|line| line.text().contains("PARAGRAPH MARKER"))
    );
    harness
        .load(&card, &book, "EPUB/end.xhtml", 0, preferences)
        .unwrap();
    assert_eq!(harness.page.chapter_title(), "Last chapter");
    original_unchanged(&card, &book, LARGE);
}

#[test]
fn corrupt_pages_indexes_and_extracted_text_rebuild_instead_of_returning_partial_content() {
    let (card, book) = fixture(MINIMAL);
    let mut harness = Harness::new();
    let preferences = ReaderPreferences::default();
    let first = harness
        .load(&card, &book, "EPUB/chapter.xhtml", 0, preferences)
        .unwrap();
    let opening = *harness.page;
    let last_index_byte = card
        .store()
        .with_app_directory(|app| {
            let cache = app.open_dir(CACHE_DIRECTORY)?;
            let file = cache.open_file_in_dir(first.key.file_name().as_str(), Mode::ReadOnly)?;
            Ok(file.length() - 1)
        })
        .unwrap();
    for offset in [0, CHAPTER_HEADER_BYTES as u32 + 8, last_index_byte] {
        corrupt(&card, first.key.file_name().as_str(), offset);
        let rebuilt = harness
            .load(&card, &book, "EPUB/chapter.xhtml", 0, preferences)
            .unwrap();
        assert_eq!(rebuilt.state, CacheState::Prepared);
        assert_eq!(*harness.page, opening);
    }
    corrupt(
        &card,
        first.key.source().file_name().as_str(),
        SOURCE_HEADER_BYTES as u32,
    );
    corrupt(&card, first.key.file_name().as_str(), 0);
    let rebuilt = harness
        .load(&card, &book, "EPUB/chapter.xhtml", 0, preferences)
        .unwrap();
    assert_eq!(rebuilt.state, CacheState::Prepared);
    assert_eq!(*harness.page, opening);
    original_unchanged(&card, &book, MINIMAL);
}

#[test]
fn every_cache_write_boundary_recovers_without_replacing_the_book() {
    let (card, book) = fixture(MINIMAL);
    let baseline = card.0.borrow().bytes.clone();
    let mut harness = Harness::new();
    let preferences = ReaderPreferences::default();
    card.0.borrow_mut().writes = 0;
    harness
        .load(&card, &book, "EPUB/chapter.xhtml", 0, preferences)
        .unwrap();
    let writes = card.0.borrow().writes;
    let expected = *harness.page;
    for limit in 0..=writes {
        let card = Card(Rc::new(RefCell::new(MemoryCard {
            bytes: baseline.clone(),
            fail_read: None,
            fail_write_after: Some(limit),
            writes: 0,
        })));
        let _ = harness.load(&card, &book, "EPUB/chapter.xhtml", 0, preferences);
        card.0.borrow_mut().fail_write_after = None;
        harness
            .load(&card, &book, "EPUB/chapter.xhtml", 0, preferences)
            .unwrap_or_else(|error| panic!("write {limit}: {error:?}"));
        assert_eq!(*harness.page, expected, "write {limit}");
        original_unchanged(&card, &book, MINIMAL);
    }
}

#[test]
fn quota_eviction_preserves_unrelated_cache_files_and_the_current_source() {
    let (card, book) = fixture(MINIMAL);
    card.store()
        .with_app_directory(|app| {
            let cache = app.open_dir(CACHE_DIRECTORY)?;
            let file = cache.open_file_in_dir("KEEP.TXT", Mode::ReadWriteCreateOrTruncate)?;
            file.write(b"unrelated")?;
            file.close()?;
            for index in 0..130 {
                let name = format!("{index:08X}.HTM");
                let file =
                    cache.open_file_in_dir(name.as_str(), Mode::ReadWriteCreateOrTruncate)?;
                file.write(b"old derived data")?;
                file.close()?;
            }
            cache.close()
        })
        .unwrap();
    let mut harness = Harness::new();
    let first = harness
        .load(
            &card,
            &book,
            "EPUB/chapter.xhtml",
            0,
            ReaderPreferences::default(),
        )
        .unwrap();
    card.store()
        .with_app_directory(|app| {
            let cache = app.open_dir(CACHE_DIRECTORY)?;
            let file = cache.open_file_in_dir("KEEP.TXT", Mode::ReadOnly)?;
            let mut bytes = [0; 9];
            assert_eq!(file.read(&mut bytes)?, bytes.len());
            assert_eq!(&bytes, b"unrelated");
            file.close()?;
            let file =
                cache.open_file_in_dir(first.key.source().file_name().as_str(), Mode::ReadOnly)?;
            assert!(file.length() > SOURCE_HEADER_BYTES as u32);
            file.close()?;
            let mut count = 0;
            cache.iterate_dir(|entry| {
                if matches!(entry.name.extension(), b"HTM" | b"PGS") {
                    count += 1;
                }
                ControlFlow::Continue(())
            })?;
            assert!(count <= 128);
            cache.close()
        })
        .unwrap();
    original_unchanged(&card, &book, MINIMAL);
}

#[test]
fn a_full_card_fails_safely_and_can_prepare_after_space_is_available() {
    let (card, book) = fixture(MINIMAL);
    card.store()
        .with_app_directory(|app| {
            let file = app.open_file_in_dir("FILL.BIN", Mode::ReadWriteCreateOrTruncate)?;
            while file.write(&[0xff; 512]).is_ok() {}
            file.close()
        })
        .unwrap();
    let mut harness = Harness::new();
    assert!(
        harness
            .load(
                &card,
                &book,
                "EPUB/chapter.xhtml",
                0,
                ReaderPreferences::default()
            )
            .is_err()
    );
    original_unchanged(&card, &book, MINIMAL);
    card.store()
        .with_app_directory(|app| app.delete_entry_in_dir("FILL.BIN"))
        .unwrap();
    assert_eq!(
        harness
            .load(
                &card,
                &book,
                "EPUB/chapter.xhtml",
                0,
                ReaderPreferences::default()
            )
            .unwrap()
            .state,
        CacheState::Prepared
    );
}
