extern crate std;

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
        let mut bytes = vec![0; 70_001 * 512];
        bytes[510..512].copy_from_slice(&[0x55, 0xaa]);
        bytes[450] = 0x0c;
        bytes[454..458].copy_from_slice(&1u32.to_le_bytes());
        bytes[458..462].copy_from_slice(&70_000u32.to_le_bytes());
        let boot = &mut bytes[512..1024];
        boot[..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
        boot[3..11].copy_from_slice(b"MSDOS5.0");
        boot[11..13].copy_from_slice(&512u16.to_le_bytes());
        boot[13] = 1;
        boot[14..16].copy_from_slice(&32u16.to_le_bytes());
        boot[16] = 2;
        boot[21] = 0xf8;
        boot[28..32].copy_from_slice(&1u32.to_le_bytes());
        boot[32..36].copy_from_slice(&70_000u32.to_le_bytes());
        boot[36..40].copy_from_slice(&550u32.to_le_bytes());
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
        info[488..492].copy_from_slice(&68_867u32.to_le_bytes());
        info[492..496].copy_from_slice(&3u32.to_le_bytes());
        info[508..512].copy_from_slice(&0xaa55_0000u32.to_le_bytes());
        bytes.copy_within(1024..1536, 8 * 512);
        for sector in [33, 583] {
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
    let transaction = ImageUploadRecord {
        name,
        length: bytes.len(),
        crc32: crc32fast::hash(bytes),
    };
    data.write_file(AppDataFile::ImageUploadTemp, bytes)
        .unwrap();
    data.copy_file_to_named(AppDataFile::ImageUploadTemp, name.as_str(), &mut [0; 32])
        .unwrap();
    data.write_file(AppDataFile::ImageUploadTemp, b"temporary bytes")
        .unwrap();
    data.write_file(AppDataFile::ImageUploadTransaction, &transaction.encode())
        .unwrap();
    card.fail_read_containing(bytes);
    let bytes_before = card.0.borrow().bytes.clone();
    assert_eq!(
        data.recover_image_upload(&mut [0; 512]),
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
    let mut record = [0; IMAGE_UPLOAD_RECORD_BYTES];
    assert_eq!(
        data.read_file(AppDataFile::ImageUploadTransaction, &mut record),
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
    data.write_file(AppDataFile::ImageUploadTransaction, b"broken")
        .unwrap();
    data.write_file(AppDataFile::ImageUploadTemp, b"partial upload")
        .unwrap();
    let before = card.0.borrow().bytes.clone();
    assert_eq!(
        data.recover_image_upload(&mut [0; 512]),
        Err(AppDataError::InvalidMetadata)
    );
    assert_eq!(
        data.scan_images::<4>().err(),
        Some(AppDataError::InvalidMetadata)
    );
    let request = UploadRequest::image(ImageName::parse("NEW.PNG").unwrap(), 8, 0);
    assert_eq!(
        data.begin_image_upload(request),
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
    data.begin_image_upload(request).unwrap();
    data.append_image_upload(bytes).unwrap();
    data.commit_image_upload(request, &mut [0; 512]).unwrap();
    data.write_selected_image(keep).unwrap();
    data.write_file(AppDataFile::ImageUploadTemp, b"uncommitted")
        .unwrap();
    data.write_file(AppDataFile::ImageUploadTransaction, &[])
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
    for file in [
        AppDataFile::ImageUploadTransaction,
        AppDataFile::ImageUploadTemp,
    ] {
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
    data.begin_image_upload(request).unwrap();
    data.append_image_upload(bytes).unwrap();
    data.commit_image_upload(request, &mut [0; 512]).unwrap();
    data.write_selected_image(keep).unwrap();
    let name = ImageName::parse("NEW.PNG").unwrap();
    let request = UploadRequest::image(name, bytes.len(), crc32fast::hash(bytes));
    data.begin_image_upload(request).unwrap();
    data.append_image_upload(bytes).unwrap();
    let snapshot = card.0.borrow().bytes.clone();
    card.0.borrow_mut().writes = 0;
    card.store()
        .app_data()
        .commit_image_upload(request, &mut [0; 512])
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
            .commit_image_upload(request, &mut [0; 512]);
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
            assert!(!data.named_file_exists(name.as_str()).unwrap());
        }
        data.begin_image_upload(request).unwrap();
        data.abort_image_upload().unwrap();
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
