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

struct CacheFixture {
    card: Card,
}

impl CacheFixture {
    const ENTRIES: usize = 33;
    const PRESERVED: [&str; 4] = ["CLOCK.BIN", "NOTES.TXT", "README", "NOTES.IMG"];

    fn new() -> Self {
        let card = Card::formatted();
        let store = card.store();
        store.ensure_layout().unwrap();
        store
            .with_app_directory(|app| {
                let cache = app.open_dir(CACHE_DIRECTORY)?;
                for name in (0..Self::ENTRIES)
                    .map(|slot| std::format!("{slot:08X}.IMG"))
                    .chain(Self::PRESERVED.map(std::string::String::from))
                {
                    let file =
                        cache.open_file_in_dir(name.as_str(), Mode::ReadWriteCreateOrTruncate)?;
                    file.write(&[0x5A; 24])?;
                    file.close()?;
                }
                cache.close()
            })
            .unwrap();
        Self { card }
    }

    fn assert_cleared(&self) {
        self.card
            .store()
            .with_app_directory(|app| {
                let cache = app.open_dir(CACHE_DIRECTORY)?;
                let mut names = Vec::new();
                cache.iterate_dir(|entry| {
                    if !entry.attributes.is_directory() {
                        names.push(entry.name);
                    }
                    ControlFlow::Continue(())
                })?;
                assert_eq!(names.len(), Self::PRESERVED.len());
                for name in Self::PRESERVED {
                    assert!(
                        names.contains(
                            &embedded_sdmmc::ShortFileName::create_from_str(name).unwrap()
                        )
                    );
                    let file = cache.open_file_in_dir(name, Mode::ReadOnly)?;
                    let mut bytes = [0; 24];
                    assert_eq!(file.read(&mut bytes)?, bytes.len());
                    assert_eq!(bytes, [0x5A; 24]);
                    file.close()?;
                }
                for slot in 0..Self::ENTRIES {
                    assert!(
                        !names.contains(
                            &embedded_sdmmc::ShortFileName::create_from_str(&std::format!(
                                "{slot:08X}.IMG"
                            ))
                            .unwrap()
                        )
                    );
                }
                cache.close()
            })
            .unwrap();
        assert_eq!(self.card.store().app_data().drop_image_cache(), Ok(0));
    }
}

#[test]
fn dropping_the_image_cache_removes_only_recognized_cache_files() {
    let fixture = CacheFixture::new();
    assert_eq!(
        fixture.card.store().app_data().drop_image_cache(),
        Ok(CacheFixture::ENTRIES)
    );
    fixture.assert_cleared();
}

#[test]
fn cache_drop_scan_failure_does_not_delete_an_incomplete_batch() {
    let fixture = CacheFixture::new();
    let baseline = fixture.card.0.borrow().bytes.clone();
    fixture.card.fail_read_containing(b"00000010IMG");
    assert_eq!(
        fixture.card.store().app_data().drop_image_cache(),
        Err(AppDataError::Filesystem(Error::DeviceError(Fault::Read)))
    );
    assert_eq!(fixture.card.0.borrow().bytes, baseline);
    fixture.card.0.borrow_mut().fail_read = None;
    assert_eq!(
        fixture.card.store().app_data().drop_image_cache(),
        Ok(CacheFixture::ENTRIES)
    );
    fixture.assert_cleared();
}

#[test]
fn cache_drop_write_failures_are_repairable_at_every_write_boundary() {
    let fixture = CacheFixture::new();
    let baseline = fixture.card.0.borrow().bytes.clone();
    fixture.card.0.borrow_mut().writes = 0;
    assert_eq!(
        fixture.card.store().app_data().drop_image_cache(),
        Ok(CacheFixture::ENTRIES)
    );
    let writes = fixture.card.0.borrow().writes;
    assert!(writes > CacheFixture::ENTRIES);
    for limit in 0..writes {
        let fixture = CacheFixture {
            card: Card(Rc::new(RefCell::new(MemoryCard {
                bytes: baseline.clone(),
                fail_read: None,
                fail_write_after: Some(limit),
                writes: 0,
            }))),
        };
        let store = fixture.card.store();
        assert_eq!(
            store.app_data().drop_image_cache(),
            Err(AppDataError::Filesystem(Error::DeviceError(Fault::Write))),
            "write {limit}"
        );
        fixture.card.0.borrow_mut().fail_write_after = None;
        let remaining = store
            .with_app_directory(|app| {
                let cache = app.open_dir(CACHE_DIRECTORY)?;
                let mut remaining = 0;
                cache.iterate_dir(|entry| {
                    if !entry.attributes.is_directory()
                        && crate::image_cache::CacheSlot::try_from(entry.name).is_ok()
                    {
                        remaining += 1;
                    }
                    ControlFlow::Continue(())
                })?;
                cache.close()?;
                Ok(remaining)
            })
            .unwrap();
        assert_eq!(
            store.app_data().drop_image_cache(),
            Ok(remaining),
            "write {limit}"
        );
        fixture.assert_cleared();
    }
}
