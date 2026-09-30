use super::bookmarks::{BookmarkBytes, BookmarkDecodeError, BookmarkFile, BookmarkRecord};
use super::*;
use crate::app::BookProgress;
use crate::storage::book_resume::BookIdentity;

fn book(name: &str, size: u32) -> BookFile {
    BookFile::new(BookFileName::try_from(name).unwrap(), size)
}

fn progress(spine_index: usize, page_index: usize, page_count: usize) -> BookProgress {
    BookProgress::new(
        spine_index,
        page_index,
        page_count,
        AppPreferences::default().reader(),
    )
    .unwrap()
}

fn bookmark_names(name: &str, size: u32) -> super::bookmarks::BookmarkSlot {
    super::bookmarks::BookmarkSlot::from(BookIdentity::from(&book(name, size)))
}

#[test]
fn progress_round_trips_per_book_and_ignores_other_identities() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    let atlas = book("0C645338.EPB", 20_895_409);
    let study = book("15116861.EPB", 6_505_283);
    assert_eq!(data.read_book_progress(&atlas), Ok(None));
    data.write_book_progress(&atlas, progress(9, 5, 15))
        .unwrap();
    data.write_book_progress(&study, progress(0, 1, 5)).unwrap();
    assert_eq!(
        data.read_book_progress(&atlas),
        Ok(Some(progress(9, 5, 15)))
    );
    assert_eq!(data.read_book_progress(&study), Ok(Some(progress(0, 1, 5))));
    assert_eq!(
        data.read_book_progress(&book("0C645338.EPB", 20_895_410)),
        Ok(None)
    );
    assert_eq!(
        data.read_book_progress(&book("0C645339.EPB", 20_895_409)),
        Ok(None)
    );
}

#[test]
fn rewritten_progress_replaces_the_previous_record_without_leaving_a_temp() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    let atlas = book("0C645338.EPB", 20_895_409);
    data.write_book_progress(&atlas, progress(9, 5, 15))
        .unwrap();
    data.write_book_progress(&atlas, progress(9, 6, 15))
        .unwrap();
    assert_eq!(
        data.read_book_progress(&atlas),
        Ok(Some(progress(9, 6, 15)))
    );
    let names = bookmark_names("0C645338.EPB", 20_895_409);
    assert_eq!(
        data.storage.with_app_directory(|app| {
            let bookmark = app.open_dir(BOOKMARK_DIRECTORY)?;
            let temp = names.name(BookmarkFile::Temporary).unwrap();
            let found = match bookmark.find_directory_entry(temp) {
                Ok(_) => true,
                Err(Error::NotFound) => false,
                Err(error) => return Err(error),
            };
            bookmark.close()?;
            Ok(found)
        }),
        Ok(false)
    );
}

#[test]
fn a_corrupt_bookmark_record_is_not_progress() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    let atlas = book("0C645338.EPB", 20_895_409);
    let names = bookmark_names("0C645338.EPB", 20_895_409);
    data.storage
        .with_app_directory(|app| {
            let bookmark = app.open_dir(BOOKMARK_DIRECTORY)?;
            let file = bookmark.open_file_in_dir(
                names.name(BookmarkFile::Primary).unwrap(),
                Mode::ReadWriteCreateOrTruncate,
            )?;
            let mut bytes = [0x5A; 288];
            bytes[..4].copy_from_slice(&0x4254_4231u32.to_le_bytes());
            file.write(&bytes)?;
            file.close()?;
            bookmark.close()
        })
        .unwrap();
    assert_eq!(
        data.read_book_progress(&atlas),
        Err(AppDataError::Bookmark(BookmarkDecodeError::Checksum))
    );
}

#[test]
fn every_failed_bookmark_write_retains_a_readable_record_after_remount() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let atlas = book("0C645338.EPB", 20_895_409);
    let old = progress(9, 5, 15);
    let new = progress(9, 6, 15);
    store.app_data().write_book_progress(&atlas, old).unwrap();
    let snapshot = card.0.borrow().bytes.clone();
    card.0.borrow_mut().writes = 0;
    card.store()
        .app_data()
        .write_book_progress(&atlas, new)
        .unwrap();
    let total_writes = card.0.borrow().writes;
    assert!(total_writes > 0);
    for limit in 0..=total_writes {
        let failing = Card(Rc::new(RefCell::new(MemoryCard {
            bytes: snapshot.clone(),
            fail_read: None,
            fail_write_after: Some(limit),
            writes: 0,
        })));
        let result = failing.store().app_data().write_book_progress(&atlas, new);
        assert_eq!(
            result.is_ok(),
            limit == total_writes,
            "write boundary {limit}"
        );
        failing.0.borrow_mut().fail_write_after = None;
        let recovered = failing
            .store()
            .app_data()
            .read_book_progress(&atlas)
            .unwrap();
        assert!(
            recovered == Some(old) || recovered == Some(new),
            "write boundary {limit}: {recovered:?}"
        );
    }
}

#[test]
fn a_valid_backup_recovers_a_corrupt_primary_record() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    let atlas = book("0C645338.EPB", 20_895_409);
    data.write_book_progress(&atlas, progress(9, 5, 15))
        .unwrap();
    data.write_book_progress(&atlas, progress(9, 6, 15))
        .unwrap();
    let names = bookmark_names("0C645338.EPB", 20_895_409);
    data.storage
        .with_app_directory(|app| {
            let bookmark = app.open_dir(BOOKMARK_DIRECTORY)?;
            let file = bookmark.open_file_in_dir(
                names.name(BookmarkFile::Primary).unwrap(),
                Mode::ReadWriteCreateOrTruncate,
            )?;
            file.write(&[0x00; 288])?;
            file.close()?;
            bookmark.close()
        })
        .unwrap();
    assert_eq!(
        data.read_book_progress(&atlas),
        Ok(Some(progress(9, 5, 15)))
    );
}

#[test]
fn a_corrupt_primary_is_not_promoted_over_the_last_valid_backup() {
    let card = Card::formatted();
    let store = card.store();
    let data = store.app_data();
    let atlas = book("0C645338.EPB", 20_895_409);
    let old = progress(9, 5, 15);
    data.write_book_progress(&atlas, old).unwrap();
    data.write_book_progress(&atlas, progress(9, 6, 15))
        .unwrap();
    let slot = bookmark_names("0C645338.EPB", 20_895_409);
    let corrupt = || {
        data.storage
            .with_app_directory(|app| {
                let directory = app.open_dir(BOOKMARK_DIRECTORY)?;
                let file = directory.open_file_in_dir(
                    slot.name(BookmarkFile::Primary).unwrap(),
                    Mode::ReadWriteCreateOrTruncate,
                )?;
                file.write(&[0; super::bookmarks::BOOKMARK_BYTES])?;
                file.close()?;
                directory.close()
            })
            .unwrap();
    };
    corrupt();
    data.write_book_progress(&atlas, progress(9, 7, 15))
        .unwrap();
    corrupt();
    assert_eq!(
        card.store().app_data().read_book_progress(&atlas),
        Ok(Some(old))
    );
}

#[test]
fn colliding_slots_keep_both_books_readable_after_rewrites_and_remount() {
    let card = Card::formatted();
    let first = book("00001B02.EPB", 373_595_746);
    let second = book("00015458.EPB", 280_029_400);
    let first_slot = super::bookmarks::BookmarkSlot::from(BookIdentity::from(&first));
    let second_slot = super::bookmarks::BookmarkSlot::from(BookIdentity::from(&second));
    assert_eq!(first_slot, second_slot);
    assert_eq!(
        first_slot.name(BookmarkFile::Primary).unwrap().base_name(),
        b"C6260A93"
    );
    assert_eq!(
        first_slot.name(BookmarkFile::Primary).unwrap().extension(),
        b"BMK"
    );
    for page in 1..4 {
        card.store()
            .app_data()
            .write_book_progress(&first, progress(1, page, 8))
            .unwrap();
        card.store()
            .app_data()
            .write_book_progress(&second, progress(2, page + 1, 9))
            .unwrap();
        assert_eq!(
            card.store().app_data().read_book_progress(&first),
            Ok(Some(progress(1, page, 8)))
        );
        assert_eq!(
            card.store().app_data().read_book_progress(&second),
            Ok(Some(progress(2, page + 1, 9)))
        );
    }
}

#[test]
fn bookmark_decode_reports_each_invalid_field() {
    let record = BookmarkRecord {
        identity: BookIdentity::from(&book("ATLAS.EPB", 42)),
        progress: progress(1, 2, 3),
    };
    let valid = BookmarkBytes::from(record);
    let offset = 8 + MAX_BOOK_NAME_BYTES;
    for (index, value, expected) in [
        (0, 0, BookmarkDecodeError::Magic),
        (
            valid.len() - 1,
            valid[valid.len() - 1] ^ 1,
            BookmarkDecodeError::Checksum,
        ),
        (4, 0, BookmarkDecodeError::IdentityLength),
        (8, 0xff, BookmarkDecodeError::IdentityInvalid),
        (offset + 12, 0, BookmarkDecodeError::ProgressInvalid),
        (offset + 16, 0xff, BookmarkDecodeError::ProgressInvalid),
    ] {
        let mut bytes = valid;
        bytes[index] = value;
        if expected != BookmarkDecodeError::Checksum {
            let end = bytes.len() - 4;
            let checksum = crc32fast::hash(&bytes[..end]);
            bytes[end..].copy_from_slice(&checksum.to_le_bytes());
        }
        assert_eq!(BookmarkRecord::try_from(&bytes), Err(expected));
    }
}

#[test]
fn exhausted_probe_sequence_does_not_overwrite_another_identity() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    let target = book("ATLAS.EPB", 42);
    let occupant = book("OTHER.EPB", 51);
    let slot = super::bookmarks::BookmarkSlot::from(BookIdentity::from(&target));
    let bytes = BookmarkBytes::from(BookmarkRecord {
        identity: BookIdentity::from(&occupant),
        progress: progress(0, 1, 8),
    });
    data.storage
        .with_app_directory(|app| {
            let directory = app.open_dir(BOOKMARK_DIRECTORY)?;
            for offset in 0..super::bookmarks::BOOKMARK_PROBES {
                let file = directory.open_file_in_dir(
                    slot.probe(offset).name(BookmarkFile::Primary).unwrap(),
                    Mode::ReadWriteCreateOrTruncate,
                )?;
                file.write(&bytes)?;
                file.close()?;
            }
            directory.close()
        })
        .unwrap();
    assert_eq!(
        data.read_book_progress(&target),
        Err(AppDataError::BookmarkSlotsExhausted)
    );
    let before = card.0.borrow().bytes.clone();
    assert_eq!(
        data.write_book_progress(&target, progress(0, 2, 8)),
        Err(AppDataError::BookmarkSlotsExhausted)
    );
    assert_eq!(card.0.borrow().bytes, before);
}
