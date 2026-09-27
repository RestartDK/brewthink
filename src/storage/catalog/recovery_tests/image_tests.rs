mod capacity;

use super::*;
use crate::{
    image::ScaleMode,
    image_cache::{CacheState, ImageSource, ImageSpec, ImageWorkspace},
    transfer::BookName,
    zip_stream::ZipValidationScratch,
};
use std::boxed::Box;

fn book_on_card() -> (Card, BookFile) {
    let card = Card::formatted();
    let store = card.store();
    let data = store.app_data();
    let bytes = include_bytes!("../../../../web/tests/fixtures/minimal.epub");
    let request = UploadRequest::book(
        BookName::parse("BOOK.EPB").unwrap(),
        bytes.len(),
        crc32fast::hash(bytes),
    );
    data.begin_upload(request).unwrap();
    data.append_upload(bytes).unwrap();
    data.commit_upload(request, &mut [0; 512]).unwrap();
    let catalog = store.scan::<16>().unwrap();
    let book = *catalog.books().next().unwrap();
    (card, book)
}

#[test]
fn image_cache_survives_remount_and_rebuilds_corruption_without_changing_book() {
    let (card, ref book) = book_on_card();
    let mut workspace = Box::new(ImageWorkspace::new());
    let mut zip = Box::new(ZipValidationScratch::new());
    let spec = ImageSpec::new(32, 48, ScaleMode::Contain).unwrap();
    let mut pixels = vec![0; spec.byte_len()];
    let first = card
        .store()
        .app_data()
        .prepare_image(
            ImageSource::Book {
                file: book,
                path: "EPUB/cover.png",
            },
            spec,
            &mut workspace,
            &mut zip,
            &mut pixels,
        )
        .unwrap();
    assert_eq!(first.state, CacheState::Prepared);
    let expected = pixels.clone();
    pixels.fill(0);
    let next = card
        .store()
        .app_data()
        .prepare_image(
            ImageSource::Book {
                file: book,
                path: "EPUB/cover.png",
            },
            spec,
            &mut workspace,
            &mut zip,
            &mut pixels,
        )
        .unwrap();
    assert_eq!(next.state, CacheState::Hit);
    assert_eq!(pixels, expected);
    let store = card.store();
    store
        .with_app_directory(|directory| {
            let cache = directory.open_dir(CACHE_DIRECTORY)?;
            let file =
                cache.open_file_in_dir(first.key.file_name().as_str(), Mode::ReadWriteAppend)?;
            file.seek_from_start(crate::image_cache::CACHE_HEADER_BYTES as u32)?;
            file.write(&[expected[0] ^ 0xff])?;
            file.close()?;
            cache.close()
        })
        .unwrap();
    let repaired = card
        .store()
        .app_data()
        .prepare_image(
            ImageSource::Book {
                file: book,
                path: "EPUB/cover.png",
            },
            spec,
            &mut workspace,
            &mut zip,
            &mut pixels,
        )
        .unwrap();
    assert_eq!(repaired.state, CacheState::Prepared);
    assert_eq!(pixels, expected);
    let original = include_bytes!("../../../../web/tests/fixtures/minimal.epub");
    let mut readback = vec![0; original.len()];
    card.store().read_at(book.name(), 0, &mut readback).unwrap();
    assert_eq!(readback, original);
    let changed = ImageSpec::new(32, 48, ScaleMode::Cover).unwrap();
    let different = card
        .store()
        .app_data()
        .prepare_image(
            ImageSource::Book {
                file: book,
                path: "EPUB/cover.png",
            },
            changed,
            &mut workspace,
            &mut zip,
            &mut pixels,
        )
        .unwrap();
    assert_eq!(different.state, CacheState::Prepared);
    assert_ne!(different.key, first.key);
}

#[test]
fn cache_write_failures_cannot_publish_partial_pixels_or_replace_the_epub() {
    let (card, ref book) = book_on_card();
    let baseline = card.0.borrow().bytes.clone();
    let mut workspace = Box::new(ImageWorkspace::new());
    let mut zip = Box::new(ZipValidationScratch::new());
    let spec = ImageSpec::new(32, 48, ScaleMode::Contain).unwrap();
    let mut pixels = vec![0; spec.byte_len()];
    card.0.borrow_mut().writes = 0;
    card.store()
        .app_data()
        .prepare_image(
            ImageSource::Book {
                file: book,
                path: "EPUB/cover.png",
            },
            spec,
            &mut workspace,
            &mut zip,
            &mut pixels,
        )
        .unwrap();
    let writes = card.0.borrow().writes;
    let expected = pixels.clone();
    let original = include_bytes!("../../../../web/tests/fixtures/minimal.epub");
    for limit in 0..=writes {
        let card = Card(Rc::new(RefCell::new(MemoryCard {
            bytes: baseline.clone(),
            fail_read: None,
            fail_write_after: Some(limit),
            writes: 0,
        })));
        let _interrupted = card.store().app_data().prepare_image(
            ImageSource::Book {
                file: book,
                path: "EPUB/cover.png",
            },
            spec,
            &mut workspace,
            &mut zip,
            &mut pixels,
        );
        card.0.borrow_mut().fail_write_after = None;
        pixels.fill(0);
        card.store()
            .app_data()
            .prepare_image(
                ImageSource::Book {
                    file: book,
                    path: "EPUB/cover.png",
                },
                spec,
                &mut workspace,
                &mut zip,
                &mut pixels,
            )
            .unwrap_or_else(|error| panic!("write {limit}: {error:?}"));
        assert_eq!(pixels, expected, "write {limit}");
        let mut readback = vec![0; original.len()];
        card.store().read_at(book.name(), 0, &mut readback).unwrap();
        assert_eq!(readback, original, "write {limit}");
    }
}
