use super::*;

#[test]
fn eviction_preserves_pinned_page_images_and_unrelated_files() {
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
    let store = card.store();
    store
        .with_app_directory(|app| {
            let cache = app.open_dir(CACHE_DIRECTORY)?;
            for index in 0..511 {
                let name = std::format!("{index:08X}.IMG");
                assert_ne!(name, first.key.file_name().as_str());
                let file =
                    cache.open_file_in_dir(name.as_str(), Mode::ReadWriteCreateOrTruncate)?;
                file.write(&[0])?;
                file.close()?;
            }
            let file = cache.open_file_in_dir("KEEP.BIN", Mode::ReadWriteCreateOrTruncate)?;
            file.write(&[0xa5; 136])?;
            file.close()?;
            let file = cache.open_file_in_dir("CLOCK.BIN", Mode::ReadWriteCreateOrTruncate)?;
            let mut cursor = [0; 8];
            cursor[..4].copy_from_slice(&(first.key.slot().0 - 1).to_le_bytes());
            let crc = crc32fast::hash(&cursor[..4]);
            cursor[4..].copy_from_slice(&crc.to_le_bytes());
            file.write(&cursor)?;
            file.close()?;
            cache.close()
        })
        .unwrap();
    let second_spec = ImageSpec::new(32, 48, ScaleMode::Cover).unwrap();
    let second = card
        .store()
        .app_data()
        .prepare_pinned_image(
            ImageSource::Book {
                file: book,
                path: "EPUB/cover.png",
            },
            second_spec,
            &mut workspace,
            &mut zip,
            &mut pixels,
            &[first.key.slot()],
        )
        .unwrap();
    assert_eq!(second.state, CacheState::Prepared);
    let retained = card
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
    assert_eq!(retained.state, CacheState::Hit);
    store
        .with_app_directory(|app| {
            let cache = app.open_dir(CACHE_DIRECTORY)?;
            let mut count = 0;
            cache.iterate_dir(|entry| {
                if entry.name.extension() == b"IMG" {
                    count += 1;
                }
                ControlFlow::Continue(())
            })?;
            assert_eq!(count, 512);
            let file = cache.open_file_in_dir("KEEP.BIN", Mode::ReadOnly)?;
            let mut bytes = [0; 136];
            assert_eq!(file.read(&mut bytes)?, bytes.len());
            assert_eq!(bytes, [0xa5; 136]);
            file.close()?;
            cache.close()
        })
        .unwrap();
}

#[test]
fn repeated_cold_preparation_reclaims_staging_and_replaced_cache_clusters() {
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
    let allocated = || {
        card.0.borrow().bytes[33 * 512..583 * 512]
            .chunks_exact(4)
            .filter(|entry| *entry != [0; 4])
            .count()
    };
    let original_allocation = allocated();
    let original_free = card.0.borrow().bytes[1024 + 488..1024 + 492].to_vec();
    for _ in 0..20 {
        card.store()
            .with_app_directory(|app| {
                let cache = app.open_dir(CACHE_DIRECTORY)?;
                let file = cache
                    .open_file_in_dir(first.key.file_name().as_str(), Mode::ReadWriteAppend)?;
                file.seek_from_start(0)?;
                file.write(b"!")?;
                file.close()?;
                cache.close()
            })
            .unwrap();
        let rebuilt = card
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
        assert_eq!(rebuilt.state, CacheState::Prepared);
        assert_eq!(allocated(), original_allocation);
        assert_eq!(
            &card.0.borrow().bytes[1024 + 488..1024 + 492],
            original_free
        );
    }
}

#[test]
fn a_full_card_fails_image_preparation_without_changing_the_source_book() {
    let (card, ref book) = book_on_card();
    let store = card.store();
    store
        .with_app_directory(|app| {
            let file = app.open_file_in_dir("FILL.BIN", Mode::ReadWriteCreateOrTruncate)?;
            while file.write(&[0xff; 512]).is_ok() {}
            file.close()
        })
        .unwrap();
    let mut workspace = Box::new(ImageWorkspace::new());
    let mut zip = Box::new(ZipValidationScratch::new());
    let spec = ImageSpec::new(32, 48, ScaleMode::Contain).unwrap();
    let mut pixels = vec![0; spec.byte_len()];
    assert!(
        store
            .app_data()
            .prepare_image(
                ImageSource::Book {
                    file: book,
                    path: "EPUB/cover.png"
                },
                spec,
                &mut workspace,
                &mut zip,
                &mut pixels
            )
            .is_err()
    );
    let original = include_bytes!("../../../../../web/tests/fixtures/minimal.epub");
    let mut bytes = vec![0; original.len()];
    store.read_at(book.name(), 0, &mut bytes).unwrap();
    assert_eq!(bytes, original);
    store
        .with_app_directory(|app| app.delete_entry_in_dir("FILL.BIN"))
        .unwrap();
    let image = card
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
    assert_eq!(image.state, CacheState::Prepared);
}
