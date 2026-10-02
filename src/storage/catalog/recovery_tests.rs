extern crate std;

mod bookmark_tests;
mod chapter_tests;
mod image_tests;

use super::*;
use embedded_sdmmc::{Block, BlockCount, BlockIdx, Timestamp};
use std::{cell::RefCell, rc::Rc, vec, vec::Vec};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Fault {
    Read,
    Write,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "injected {self:?} failure")
    }
}

impl core::error::Error for Fault {}

struct MemoryCard {
    bytes: Vec<u8>,
    fail_read: Option<u32>,
    fail_write_after: Option<usize>,
    writes: usize,
}

#[derive(Clone)]
struct Card(Rc<RefCell<MemoryCard>>);

impl Card {
    fn formatted() -> Self {
        Self::formatted_with_layout(70_000, 550)
    }

    fn formatted_with_layout(sectors: u32, fat_sectors: u32) -> Self {
        let clusters = sectors - 32 - 2 * fat_sectors;
        assert!(clusters >= 65_525 && fat_sectors * 128 >= clusters + 2);
        let mut bytes = vec![0; (sectors as usize + 1) * 512];
        bytes[510..512].copy_from_slice(&[0x55, 0xaa]);
        bytes[450] = 0x0c;
        bytes[454..458].copy_from_slice(&1u32.to_le_bytes());
        bytes[458..462].copy_from_slice(&sectors.to_le_bytes());
        let boot = &mut bytes[512..1024];
        boot[..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
        boot[3..11].copy_from_slice(b"MSDOS5.0");
        boot[11..13].copy_from_slice(&512u16.to_le_bytes());
        boot[13] = 1;
        boot[14..16].copy_from_slice(&32u16.to_le_bytes());
        boot[16] = 2;
        boot[21] = 0xf8;
        boot[28..32].copy_from_slice(&1u32.to_le_bytes());
        boot[32..36].copy_from_slice(&sectors.to_le_bytes());
        boot[36..40].copy_from_slice(&fat_sectors.to_le_bytes());
        boot[44..48].copy_from_slice(&2u32.to_le_bytes());
        boot[48..50].copy_from_slice(&1u16.to_le_bytes());
        boot[50..52].copy_from_slice(&6u16.to_le_bytes());
        boot[64] = 0x80;
        boot[66] = 0x29;
        boot[71..82].copy_from_slice(b"TEST VOLUME");
        boot[82..90].copy_from_slice(b"FAT32   ");
        boot[510..512].copy_from_slice(&[0x55, 0xaa]);
        bytes.copy_within(512..1024, 7 * 512);
        let info = &mut bytes[1024..1536];
        info[..4].copy_from_slice(&0x4161_5252u32.to_le_bytes());
        info[484..488].copy_from_slice(&0x6141_7272u32.to_le_bytes());
        info[488..492].copy_from_slice(&(clusters - 1).to_le_bytes());
        info[492..496].copy_from_slice(&3u32.to_le_bytes());
        info[508..512].copy_from_slice(&0xaa55_0000u32.to_le_bytes());
        bytes.copy_within(1024..1536, 8 * 512);
        for sector in [33, 33 + fat_sectors as usize] {
            for (index, entry) in [0x0fff_fff8u32, 0x0fff_ffff, 0x0fff_ffff]
                .iter()
                .enumerate()
            {
                let offset = sector * 512 + index * 4;
                bytes[offset..offset + 4].copy_from_slice(&entry.to_le_bytes());
            }
        }
        Self(Rc::new(RefCell::new(MemoryCard {
            bytes,
            fail_read: None,
            fail_write_after: None,
            writes: 0,
        })))
    }

    fn store(&self) -> FatStorage<Self, Clock> {
        FatStorage::new(self.clone(), Clock)
    }

    fn fail_read_containing(&self, prefix: &[u8]) {
        let mut card = self.0.borrow_mut();
        let mut matches = card
            .bytes
            .chunks_exact(512)
            .enumerate()
            .filter(|(_, sector)| sector.windows(prefix.len()).any(|bytes| bytes == prefix));
        let sector = matches.next().expect("fixture data exists").0 as u32;
        assert!(matches.next().is_none(), "fault target must be unique");
        card.fail_read = Some(sector);
    }
}

impl BlockDevice for Card {
    type Error = Fault;

    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), Fault> {
        let card = self.0.borrow();
        for (offset, block) in blocks.iter_mut().enumerate() {
            let sector = start.0 + offset as u32;
            if card.fail_read == Some(sector) {
                return Err(Fault::Read);
            }
            let offset = sector as usize * 512;
            block
                .contents
                .copy_from_slice(&card.bytes[offset..offset + 512]);
        }
        Ok(())
    }

    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Fault> {
        let mut card = self.0.borrow_mut();
        for (offset, block) in blocks.iter().enumerate() {
            if card
                .fail_write_after
                .is_some_and(|limit| card.writes >= limit)
            {
                return Err(Fault::Write);
            }
            card.writes += 1;
            let offset = (start.0 as usize + offset) * 512;
            card.bytes[offset..offset + 512].copy_from_slice(&block.contents);
        }
        Ok(())
    }

    fn num_blocks(&self) -> Result<BlockCount, Fault> {
        Ok(BlockCount((self.0.borrow().bytes.len() / 512) as u32))
    }
}

struct Clock;

impl TimeSource for Clock {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

#[test]
fn missing_preferences_are_absent_but_device_errors_are_not() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    assert_eq!(store.app_data().read_preferences(), Ok(None));
    card.0.borrow_mut().fail_read = Some(0);
    assert_eq!(
        card.store().app_data().read_preferences(),
        Err(AppDataError::Filesystem(Error::DeviceError(Fault::Read)))
    );
}

#[test]
fn malformed_preferences_are_not_missing_preferences() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    data.write_file(AppDataFile::Preferences, b"broken")
        .unwrap();
    assert_eq!(data.read_preferences(), Err(AppDataError::InvalidMetadata));
}

#[test]
fn valid_preference_backup_recovers_a_corrupt_primary() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    let preferences = AppPreferences::default();
    data.write_preferences(preferences).unwrap();
    data.copy_file(
        AppDataFile::Preferences,
        AppDataFile::PreferencesBackup,
        &mut [0; 32],
    )
    .unwrap();
    data.write_file(AppDataFile::Preferences, b"broken")
        .unwrap();
    assert_eq!(data.read_preferences(), Ok(Some(preferences)));
}

#[test]
fn a_corrupt_selection_does_not_replace_the_valid_backup() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    let old = ImageName::parse("OLD.PNG").unwrap();
    let new = ImageName::parse("NEW.PNG").unwrap();
    let old_bytes = encode_image_selection(old);
    data.write_file(AppDataFile::ImageSelectionBackup, &old_bytes)
        .unwrap();
    data.write_file(AppDataFile::ImageSelection, b"broken")
        .unwrap();
    data.write_selected_image(new).unwrap();
    let mut backup = [0; IMAGE_SELECTION_BYTES];
    assert_eq!(
        data.read_file(AppDataFile::ImageSelectionBackup, &mut backup),
        Ok(backup.len())
    );
    assert_eq!(backup, old_bytes);
}

#[test]
fn an_unreadable_upload_target_is_not_deleted() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    let bytes = b"\x89PNG\r\n\x1a\nrecovery-payload";
    let name = ImageName::parse("TEST.PNG").unwrap();
    let transaction = UploadRecord {
        target: UploadTarget::Image(name),
        length: bytes.len(),
        crc32: crc32fast::hash(bytes),
    };
    data.write_file(AppDataFile::UploadTemp, bytes).unwrap();
    data.copy_file_to_named(
        AppDataFile::UploadTemp,
        UploadTarget::Image(name),
        &mut [0; 32],
    )
    .unwrap();
    data.write_file(AppDataFile::UploadTemp, b"temporary bytes")
        .unwrap();
    data.write_file(AppDataFile::UploadTransaction, &transaction.encode())
        .unwrap();
    card.fail_read_containing(bytes);
    let bytes_before = card.0.borrow().bytes.clone();
    assert_eq!(
        data.recover_upload(&mut [0; 512]),
        Err(AppDataError::Filesystem(Error::DeviceError(Fault::Read)))
    );
    assert!(card.0.borrow().bytes == bytes_before);
    card.0.borrow_mut().fail_read = None;
    let mut output = [0; 64];
    assert_eq!(
        data.read_named_file(name.as_str(), &mut output),
        Ok(bytes.len())
    );
    assert_eq!(&output[..bytes.len()], bytes);
    let mut record = [0; UPLOAD_RECORD_BYTES];
    assert_eq!(
        data.read_file(AppDataFile::UploadTransaction, &mut record),
        Ok(record.len())
    );
    assert_eq!(record, transaction.encode());
}

#[test]
fn unreadable_primary_does_not_silently_fall_back() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    let primary = encode_image_selection(ImageName::parse("NEW.PNG").unwrap());
    let backup = encode_image_selection(ImageName::parse("OLD.PNG").unwrap());
    data.write_file(AppDataFile::ImageSelection, &primary)
        .unwrap();
    data.write_file(AppDataFile::ImageSelectionBackup, &backup)
        .unwrap();
    card.fail_read_containing(&primary);
    assert_eq!(
        data.read_selected_image(),
        Err(AppDataError::Filesystem(Error::DeviceError(Fault::Read)))
    );
}

#[test]
fn corrupt_upload_journal_is_preserved_without_changing_stored_bytes() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    data.write_file(AppDataFile::UploadTransaction, b"broken")
        .unwrap();
    data.write_file(AppDataFile::UploadTemp, b"partial upload")
        .unwrap();
    let before = card.0.borrow().bytes.clone();
    assert_eq!(
        data.recover_upload(&mut [0; 512]),
        Err(AppDataError::InvalidMetadata)
    );
    assert_eq!(
        data.scan_images::<4>().err(),
        Some(AppDataError::InvalidMetadata)
    );
    let request = UploadRequest::image(ImageName::parse("NEW.PNG").unwrap(), 8, 0);
    assert_eq!(
        data.begin_upload(request),
        Err(AppDataError::InvalidMetadata)
    );
    assert!(card.0.borrow().bytes == before);
}

#[test]
fn every_failed_selection_write_retains_a_readable_record_after_remount() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let old = ImageName::parse("OLD.PNG").unwrap();
    let new = ImageName::parse("NEW.PNG").unwrap();
    store.app_data().write_selected_image(old).unwrap();
    let snapshot = card.0.borrow().bytes.clone();
    card.0.borrow_mut().writes = 0;
    card.store().app_data().write_selected_image(new).unwrap();
    let total_writes = card.0.borrow().writes;
    assert!(total_writes > 0);
    for limit in 0..=total_writes {
        let failing = Card(Rc::new(RefCell::new(MemoryCard {
            bytes: snapshot.clone(),
            fail_read: None,
            fail_write_after: Some(limit),
            writes: 0,
        })));
        let result = failing.store().app_data().write_selected_image(new);
        assert_eq!(
            result.is_ok(),
            limit == total_writes,
            "write boundary {limit}"
        );
        failing.0.borrow_mut().fail_write_after = None;
        let recovered = failing.store().app_data().read_selected_image().unwrap();
        assert!(
            recovered == Some(old) || recovered == Some(new),
            "write boundary {limit}: {recovered:?}"
        );
    }
}

#[test]
fn both_book_scan_apis_distinguish_missing_directories_from_io_failure() {
    let card = Card::formatted();
    assert_eq!(card.store().scan::<4>().unwrap().books().count(), 0);
    let mut catalog = BookCatalog::<4>::empty();
    catalog.inspect("STALE.EPUB", 100);
    card.store().scan_into(&mut catalog).unwrap();
    assert_eq!(catalog.books().count(), 0);

    card.store().ensure_layout().unwrap();
    card.fail_read_containing(b"BOOKS      ");
    assert!(matches!(
        card.store().scan::<4>(),
        Err(Error::DeviceError(Fault::Read))
    ));
    catalog.inspect("STALE.EPUB", 100);
    assert_eq!(
        card.store().scan_into(&mut catalog),
        Err(Error::DeviceError(Fault::Read))
    );
    assert_eq!(catalog.books().count(), 0);
}

#[test]
fn an_empty_upload_journal_has_no_authority_over_named_images() {
    let card = Card::formatted();
    let store = card.store();
    let data = store.app_data();
    let bytes = b"\x89PNG\r\n\x1a\nkeep-image";
    let keep = ImageName::parse("KEEP.PNG").unwrap();
    let request = UploadRequest::image(keep, bytes.len(), crc32fast::hash(bytes));
    data.begin_upload(request).unwrap();
    data.append_upload(bytes).unwrap();
    data.commit_upload(request, &mut [0; 512]).unwrap();
    data.write_selected_image(keep).unwrap();
    data.write_file(AppDataFile::UploadTemp, b"uncommitted")
        .unwrap();
    data.write_file(AppDataFile::UploadTransaction, &[])
        .unwrap();

    let catalog = data.scan_images::<4>().unwrap();
    assert_eq!(catalog.len(), 1);
    assert!(catalog.position(keep).is_some());
    assert_eq!(data.read_selected_image(), Ok(Some(keep)));
    let mut output = [0; 64];
    assert_eq!(
        data.read_named_file(keep.as_str(), &mut output),
        Ok(bytes.len())
    );
    assert_eq!(&output[..bytes.len()], bytes);
    for file in [AppDataFile::UploadTransaction, AppDataFile::UploadTemp] {
        assert!(matches!(
            data.read_file(file, &mut output),
            Err(AppDataError::Filesystem(Error::NotFound))
        ));
    }
}

#[test]
fn every_failed_image_commit_recovers_without_blocking_existing_images() {
    let card = Card::formatted();
    let store = card.store();
    let data = store.app_data();
    let bytes = b"\x89PNG\r\n\x1a\nkeep-image";
    let keep = ImageName::parse("KEEP.PNG").unwrap();
    let request = UploadRequest::image(keep, bytes.len(), crc32fast::hash(bytes));
    data.begin_upload(request).unwrap();
    data.append_upload(bytes).unwrap();
    data.commit_upload(request, &mut [0; 512]).unwrap();
    data.write_selected_image(keep).unwrap();
    let name = ImageName::parse("NEW.PNG").unwrap();
    let request = UploadRequest::image(name, bytes.len(), crc32fast::hash(bytes));
    data.begin_upload(request).unwrap();
    data.append_upload(bytes).unwrap();
    let snapshot = card.0.borrow().bytes.clone();
    card.0.borrow_mut().writes = 0;
    card.store()
        .app_data()
        .commit_upload(request, &mut [0; 512])
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
        let result = failing
            .store()
            .app_data()
            .commit_upload(request, &mut [0; 512]);
        assert_eq!(
            result.is_ok(),
            limit == total_writes,
            "write boundary {limit}"
        );
        failing.0.borrow_mut().fail_write_after = None;
        let remounted = failing.store();
        let data = remounted.app_data();
        let catalog = data
            .scan_images::<4>()
            .unwrap_or_else(|error| panic!("write boundary {limit}: {error:?}"));
        assert!(catalog.position(keep).is_some(), "write boundary {limit}");
        assert_eq!(
            data.read_selected_image(),
            Ok(Some(keep)),
            "write boundary {limit}"
        );
        let mut output = [0; 64];
        assert_eq!(
            data.read_named_file(keep.as_str(), &mut output),
            Ok(bytes.len())
        );
        assert_eq!(&output[..bytes.len()], bytes);
        if catalog.position(name).is_some() {
            assert_eq!(
                data.read_named_file(name.as_str(), &mut output),
                Ok(bytes.len())
            );
            assert_eq!(&output[..bytes.len()], bytes);
        } else {
            assert!(!data.named_file_exists(UploadTarget::Image(name)).unwrap());
        }
        data.begin_upload(request).unwrap();
        data.abort_upload().unwrap();
    }
}

#[test]
fn books_stream_to_books_and_retry_without_overwriting() {
    use crate::transfer::{BookName, FileTransfer};
    let card = Card::formatted();
    let store = card.store();
    let mut data = store.app_data();
    let name = BookName::parse("BOOK.EPB").unwrap();
    let mut bytes = vec![42; 130 * 1024];
    bytes[..4].copy_from_slice(b"PK\x03\x04");
    let request = UploadRequest::book(name, bytes.len(), crc32fast::hash(&bytes));
    for _ in 0..2 {
        let mut transfer = FileTransfer::new();
        transfer.begin(request, &mut data).unwrap();
        for chunk in bytes.chunks(4096) {
            transfer.append(chunk, &mut data).unwrap();
        }
        transfer.finish(&mut data, &mut [0; 4096]).unwrap();
    }
    let remounted = card.store();
    let catalog = remounted.scan::<16>().unwrap();
    assert_eq!(catalog.len(), 1);
    let file = catalog.books().next().unwrap();
    assert_eq!(file.name().as_str(), "BOOK.EPB");
    let mut readback = vec![0; bytes.len()];
    assert_eq!(
        remounted.read_at(file.name(), 0, &mut readback).unwrap(),
        bytes.len()
    );
    assert_eq!(readback, bytes);
    assert_eq!(remounted.app_data().scan_images::<16>().unwrap().len(), 0);
    let changed = UploadRequest::book(name, bytes.len(), request.crc32() ^ 1);
    let before_verification = card.0.borrow().bytes.clone();
    data.verify_upload(request, &mut [0; 4096]).unwrap();
    assert_eq!(
        data.verify_upload(changed, &mut [0; 4096]),
        Err(AppDataError::ChecksumMismatch)
    );
    let missing = UploadRequest::book(
        BookName::parse("MISSING.EPB").unwrap(),
        request.length(),
        request.crc32(),
    );
    assert!(matches!(
        data.verify_upload(missing, &mut [0; 4096]),
        Err(AppDataError::Filesystem(Error::NotFound))
    ));
    assert_eq!(card.0.borrow().bytes, before_verification);
    assert_eq!(data.begin_upload(changed), Err(AppDataError::TargetExists));
    data.verify_named_file(
        request.target(),
        request.length(),
        request.crc32(),
        &mut [0; 512],
    )
    .unwrap();
}

#[test]
fn every_failed_book_commit_preserves_books_and_images_after_remount() {
    use crate::transfer::BookName;
    let card = Card::formatted();
    let store = card.store();
    let data = store.app_data();
    let image = b"\x89PNG\r\n\x1a\nkeep-image";
    let book = b"PK\x03\x04keep-book";
    let image_request = UploadRequest::image(
        ImageName::parse("KEEP.PNG").unwrap(),
        image.len(),
        crc32fast::hash(image),
    );
    let book_request = UploadRequest::book(
        BookName::parse("KEEP.EPB").unwrap(),
        book.len(),
        crc32fast::hash(book),
    );
    for (request, bytes) in [(image_request, &image[..]), (book_request, &book[..])] {
        data.begin_upload(request).unwrap();
        data.append_upload(bytes).unwrap();
        data.commit_upload(request, &mut [0; 512]).unwrap();
    }
    let request = UploadRequest::book(
        BookName::parse("NEW.EPB").unwrap(),
        book.len(),
        crc32fast::hash(book),
    );
    data.begin_upload(request).unwrap();
    data.append_upload(book).unwrap();
    let snapshot = card.0.borrow().bytes.clone();
    card.0.borrow_mut().writes = 0;
    data.commit_upload(request, &mut [0; 512]).unwrap();
    let total_writes = card.0.borrow().writes;
    for limit in 0..=total_writes {
        let failing = Card(Rc::new(RefCell::new(MemoryCard {
            bytes: snapshot.clone(),
            fail_read: None,
            fail_write_after: Some(limit),
            writes: 0,
        })));
        let result = failing
            .store()
            .app_data()
            .commit_upload(request, &mut [0; 512]);
        assert_eq!(
            result.is_ok(),
            limit == total_writes,
            "write boundary {limit}"
        );
        failing.0.borrow_mut().fail_write_after = None;
        let remounted = failing.store();
        let data = remounted.app_data();
        data.recover_upload(&mut [0; 512]).unwrap();
        for kept in [image_request, book_request] {
            data.verify_named_file(kept.target(), kept.length(), kept.crc32(), &mut [0; 512])
                .unwrap();
        }
        if data.named_file_exists(request.target()).unwrap() {
            data.verify_named_file(
                request.target(),
                request.length(),
                request.crc32(),
                &mut [0; 512],
            )
            .unwrap();
        }
        data.begin_upload(request).unwrap();
        data.abort_upload().unwrap();
    }
}

#[test]
fn every_failed_preference_write_retains_a_readable_record_after_remount() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let old = AppPreferences::default();
    let new = AppPreferences::new(old.reader(), crate::app::SleepScreenMode::Custom);
    assert_ne!(old, new);
    store.app_data().write_preferences(old).unwrap();
    let snapshot = card.0.borrow().bytes.clone();
    card.0.borrow_mut().writes = 0;
    card.store().app_data().write_preferences(new).unwrap();
    let total_writes = card.0.borrow().writes;
    assert!(total_writes > 0);
    for limit in 0..=total_writes {
        let failing = Card(Rc::new(RefCell::new(MemoryCard {
            bytes: snapshot.clone(),
            fail_read: None,
            fail_write_after: Some(limit),
            writes: 0,
        })));
        let result = failing.store().app_data().write_preferences(new);
        assert_eq!(
            result.is_ok(),
            limit == total_writes,
            "write boundary {limit}"
        );
        failing.0.borrow_mut().fail_write_after = None;
        let recovered = failing.store().app_data().read_preferences().unwrap();
        assert!(
            recovered == Some(old) || recovered == Some(new),
            "write boundary {limit}: {recovered:?}"
        );
    }
}

#[test]
fn deleting_a_book_removes_it_from_the_catalog_and_keeps_the_rest() {
    use embedded_sdmmc::Mode;

    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    for name in ["ALPHA.EPB", "BRAVO.EPB"] {
        store
            .with_directory(BOOK_DIRECTORY, |directory| {
                let file = directory.open_file_in_dir(name, Mode::ReadWriteCreateOrTruncate)?;
                file.write(b"book")?;
                file.close()
            })
            .unwrap();
    }
    assert_eq!(store.scan::<4>().unwrap().len(), 2);

    assert_eq!(store.delete_book("ALPHA.EPB"), Ok(true));
    let catalog = store.scan::<4>().unwrap();
    assert_eq!(
        catalog
            .books()
            .map(|book| book.name().as_str())
            .collect::<Vec<_>>(),
        ["BRAVO.EPB"]
    );
    assert_eq!(store.delete_book("ALPHA.EPB"), Ok(false));
}

#[test]
fn filesystem_errors_keep_their_source_chain() {
    use core::error::Error as _;
    let error = AppDataError::Filesystem(Error::DeviceError(Fault::Read));
    assert_eq!(
        error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<Fault>(),
        Some(&Fault::Read)
    );
}

fn resume_words(page_index: usize) -> [u32; RESUME_WORDS] {
    use crate::app::{BookId, BookOrigin, ResumePoint};

    let name = BookFileName::try_from("Alpha.epub").unwrap();
    let books = [Some(BookFile::new(name, 1234))];
    SavedResume::capture(
        ResumePoint::Reader {
            book: BookId::new(0),
            spine_index: 2,
            page_index,
            origin: BookOrigin::Books,
        },
        AppPreferences::default(),
        &books,
    )
    .unwrap()
    .encode()
}

#[test]
fn missing_last_resume_is_absent() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    assert_eq!(store.app_data().read_last_resume(), Ok(None));
}

#[test]
fn unchanged_last_resume_is_not_rewritten() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    let old = resume_words(3);
    let new = resume_words(4);
    data.write_last_resume(old).unwrap();
    data.write_last_resume(new).unwrap();
    assert_eq!(
        data.read_records(&[AppDataFile::LastResumeBackup], decode_last_resume),
        Ok(Some(old))
    );
    data.write_last_resume(new).unwrap();
    assert_eq!(
        data.read_records(&[AppDataFile::LastResumeBackup], decode_last_resume),
        Ok(Some(old)),
        "a repeated write must not replace the backup"
    );
    assert_eq!(data.read_last_resume(), Ok(Some(new)));
}

#[test]
fn malformed_last_resume_is_not_a_missing_record() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    data.write_file(AppDataFile::LastResume, b"broken").unwrap();
    assert_eq!(data.read_last_resume(), Err(AppDataError::InvalidMetadata));
}

#[test]
fn valid_last_resume_backup_recovers_a_corrupt_primary() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let data = store.app_data();
    let words = resume_words(3);
    data.write_last_resume(words).unwrap();
    data.copy_file(
        AppDataFile::LastResume,
        AppDataFile::LastResumeBackup,
        &mut [0; 32],
    )
    .unwrap();
    data.write_file(AppDataFile::LastResume, b"broken").unwrap();
    assert_eq!(data.read_last_resume(), Ok(Some(words)));
}

#[test]
fn every_failed_last_resume_write_retains_a_readable_record_after_remount() {
    let card = Card::formatted();
    let store = card.store();
    store.ensure_layout().unwrap();
    let old = resume_words(3);
    let new = resume_words(4);
    store.app_data().write_last_resume(old).unwrap();
    let snapshot = card.0.borrow().bytes.clone();
    card.0.borrow_mut().writes = 0;
    card.store().app_data().write_last_resume(new).unwrap();
    let total_writes = card.0.borrow().writes;
    assert!(total_writes > 0);
    for limit in 0..=total_writes {
        let failing = Card(Rc::new(RefCell::new(MemoryCard {
            bytes: snapshot.clone(),
            fail_read: None,
            fail_write_after: Some(limit),
            writes: 0,
        })));
        let result = failing.store().app_data().write_last_resume(new);
        assert_eq!(
            result.is_ok(),
            limit == total_writes,
            "write boundary {limit}"
        );
        failing.0.borrow_mut().fail_write_after = None;
        let recovered = failing.store().app_data().read_last_resume().unwrap();
        assert!(
            recovered == Some(old) || recovered == Some(new),
            "write boundary {limit}"
        );
    }
}
