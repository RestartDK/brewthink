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

fn bookmark_names(name: &str, size: u32) -> super::bookmarks::BookmarkNames {
    super::bookmarks::BookmarkNames::new(BookIdentity::from(&book(name, size)))
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
            let temp = core::str::from_utf8(&names.temp).unwrap();
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
                core::str::from_utf8(&names.stored).unwrap(),
                Mode::ReadWriteCreateOrTruncate,
            )?;
            let mut bytes = [0x5A; 288];
            bytes[..4].copy_from_slice(&0x4254_4231u32.to_le_bytes());
            file.write(&bytes)?;
            file.close()?;
            bookmark.close()
        })
        .unwrap();
    assert_eq!(data.read_book_progress(&atlas), Ok(None));
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
                core::str::from_utf8(&names.stored).unwrap(),
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
