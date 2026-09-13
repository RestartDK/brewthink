extern crate std;

use std::{format, vec::Vec};

use super::*;
use crate::app::{App, AppEffect, AppInput, AppView, Direction, PageTarget};

fn file(name: &str, size: u32) -> Option<BookFile> {
    Some(BookFile::new(BookFileName::try_from(name).unwrap(), size))
}

#[test]
fn standard_conversions_preserve_filename_bounds_and_identity() {
    let maximum = "å".repeat(MAX_BOOK_NAME_BYTES / 2);
    let name = BookFileName::try_from(maximum.as_str()).unwrap();
    assert_eq!(name.as_str(), maximum);
    let via_try_into: BookFileName = maximum.as_str().try_into().unwrap();
    assert_eq!(via_try_into, name);
    assert!(BookFileName::try_from("").is_err());
    assert!(BookFileName::try_from(format!("{maximum}x").as_str()).is_err());
    assert_eq!(BookFileName::try_from("a").unwrap().as_str(), "a");

    let book = BookFile::new(name, 123);
    let identity: BookIdentity = (&book).into();
    assert_eq!(identity, BookIdentity::from(&book));
    assert_eq!(identity.resolve(&[Some(book)]), Ok(BookId::new(0)));
    assert_eq!(
        identity.resolve(&[Some(BookFile::new(name, 124))]),
        Err(ResumeError::Missing)
    );
    assert_eq!(
        identity.resolve(&[Some(book), Some(book)]),
        Err(ResumeError::Ambiguous)
    );
}

fn reader(book: usize) -> ResumePoint {
    ResumePoint::Reader {
        book: BookId::new(book),
        spine_index: 2,
        page_index: 3,
        origin: BookOrigin::Files,
    }
}

fn saved(resume: ResumePoint, books: &[Option<BookFile>]) -> SavedResume {
    let captured = SavedResume::capture(resume, AppPreferences::default(), books).unwrap();
    SavedResume::decode(&captured.encode()).unwrap()
}

fn load_effect(saved: &SavedResume, books: &[Option<BookFile>]) -> Result<AppEffect, ResumeError> {
    let resume = saved.resolve(books)?;
    Ok(App::from_resume(books.len(), saved.preferences(), resume)
        .unwrap()
        .1)
}

#[test]
fn persistent_resume_keeps_selected_title_after_catalog_reorder() {
    let original = [file("Alpha.epub", 100), file("Bravo.epub", 200)];
    let reordered = [original[1], original[0]];
    let retained = saved(reader(0), &original);
    let AppEffect::LoadChapter { book, .. } = load_effect(&retained, &reordered).unwrap() else {
        panic!("expected chapter load")
    };
    assert_eq!(reordered[book.index()], original[0]);
    assert_eq!(retained.resolve(&reordered), Ok(reader(1)));
}

#[test]
fn skipped_unreadable_entries_do_not_shift_resume_to_another_book() {
    let original = [
        file("Bad.epub", 50),
        file("Alpha.epub", 100),
        file("Bravo.epub", 200),
    ];
    let retained = saved(reader(1), &original);
    let compacted = [original[1], original[2]];
    assert_eq!(retained.resolve(&compacted), Ok(reader(0)));
    let holes = [None, original[1], original[2]];
    assert_eq!(retained.resolve(&holes), Ok(reader(1)));
    assert_eq!(
        load_effect(&retained, &[original[0], original[2]]),
        Err(ResumeError::Missing)
    );
}

#[test]
fn insertion_and_removal_preserve_only_the_matching_book() {
    let original = [file("Alpha.epub", 100), file("Bravo.epub", 200)];
    let retained = saved(reader(1), &original);
    let inserted = [file("New.epub", 80), original[0], original[1]];
    assert_eq!(retained.resolve(&inserted), Ok(reader(2)));
    assert_eq!(retained.resolve(&[original[1]]), Ok(reader(0)));
    let replaced_slot = [original[0], file("Unrelated.epub", 200)];
    assert_eq!(
        load_effect(&retained, &replaced_slot),
        Err(ResumeError::Missing)
    );
    assert_eq!(load_effect(&retained, &[]), Err(ResumeError::Missing));
}

#[test]
fn every_catalog_permutation_resolves_the_same_file() {
    let original = [
        file("A.epub", 100),
        file("B.epub", 100),
        file("C.epub", 100),
    ];
    for selected in 0..original.len() {
        let retained = saved(reader(selected), &original);
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let reordered = order.map(|index| original[index]);
            let AppEffect::LoadChapter { book, .. } = load_effect(&retained, &reordered).unwrap()
            else {
                panic!("expected chapter load")
            };
            assert_eq!(reordered[book.index()], original[selected]);
        }
    }
}

#[test]
fn duplicate_identity_is_ambiguous_on_capture_and_restore() {
    let original = [file("Alpha.epub", 100)];
    let duplicates = [original[0], file("Bravo.epub", 200), original[0]];
    let retained = saved(reader(0), &original);
    assert_eq!(
        load_effect(&retained, &duplicates),
        Err(ResumeError::Ambiguous)
    );
    assert_eq!(
        SavedResume::capture(reader(0), AppPreferences::default(), &duplicates),
        Err(ResumeError::Ambiguous)
    );
    assert_eq!(
        SavedResume::capture(reader(9), AppPreferences::default(), &original),
        Err(ResumeError::Missing)
    );
}

#[test]
fn filenames_with_a_32_bit_hash_collision_are_distinct_identities() {
    let fnv32 = |name: &str| {
        name.bytes().fold(0x811C_9DC5u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        })
    };
    let first = "costarring.epub";
    let second = "liquid.epub";
    assert_eq!(fnv32(first), fnv32(second));
    let original = [file(first, 100), file(second, 100)];
    let retained = saved(reader(0), &original);
    assert_eq!(
        load_effect(&retained, &[original[1]]),
        Err(ResumeError::Missing)
    );
    assert_eq!(retained.resolve(&[original[1], original[0]]), Ok(reader(1)));
}

#[test]
fn complete_names_are_compared_without_truncation_or_unicode_normalization() {
    let prefix = "a".repeat(MAX_BOOK_NAME_BYTES - 6);
    let first = format!("{prefix}a.epub");
    let second = format!("{prefix}b.epub");
    assert_eq!(first.len(), MAX_BOOK_NAME_BYTES);
    let original = [file(&first, 100), file(&second, 100)];
    let retained = saved(reader(0), &original);
    assert_eq!(retained.resolve(&[original[1]]), Err(ResumeError::Missing));
    assert_eq!(retained.resolve(&[original[1], original[0]]), Ok(reader(1)));
    let unicode = [file("Caf\u{e9}.epub", 100)];
    let retained = saved(reader(0), &unicode);
    assert_eq!(retained.resolve(&unicode), Ok(reader(0)));
    assert_eq!(
        retained.resolve(&[file("Cafe\u{301}.epub", 100)]),
        Err(ResumeError::Missing)
    );
}

#[test]
fn rename_and_size_change_intentionally_do_not_restore() {
    let original = [file("Alpha.epub", 100)];
    let retained = saved(reader(0), &original);
    for replacement in [
        file("Renamed.epub", 100),
        file("alpha.epub", 100),
        file("Alpha.epub", 101),
    ] {
        assert_eq!(
            load_effect(&retained, &[replacement]),
            Err(ResumeError::Missing)
        );
    }
    assert_eq!(retained.resolve(&[file("Alpha.epub", 100)]), Ok(reader(0)));
}

#[test]
fn books_and_files_selections_use_book_identity_too() {
    let original = [file("Alpha.epub", 100), file("Bravo.epub", 200)];
    let reordered = [original[1], original[0]];
    for (before, after) in [
        (
            ResumePoint::Books {
                selected: Some(BookId::new(0)),
            },
            ResumePoint::Books {
                selected: Some(BookId::new(1)),
            },
        ),
        (
            ResumePoint::Files {
                selected: Some(FileId::new(0)),
            },
            ResumePoint::Files {
                selected: Some(FileId::new(1)),
            },
        ),
    ] {
        let retained = saved(before, &original);
        assert_eq!(retained.resolve(&reordered), Ok(after));
        assert_eq!(retained.resolve(&[original[1]]), Err(ResumeError::Missing));
        let (mut app, effect) = App::from_resume(
            reordered.len(),
            retained.preferences(),
            retained.resolve(&reordered).unwrap(),
        )
        .unwrap();
        assert_eq!(effect, AppEffect::Render);
        assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
        assert!(matches!(app.view(), AppView::BookCover { book, .. } if book == BookId::new(1)));
        assert_eq!(
            app.input(AppInput::Confirm),
            AppEffect::LoadChapter {
                book: BookId::new(1),
                spine_index: 0,
                target: PageTarget::First,
            }
        );
    }
}

#[test]
fn files_image_selection_cannot_become_a_book_when_books_are_inserted() {
    let original = [file("Alpha.epub", 100)];
    let retained = saved(
        ResumePoint::Files {
            selected: Some(FileId::new(1)),
        },
        &original,
    );
    let inserted = [file("New.epub", 200), original[0]];
    for books in [&inserted[..], &[][..]] {
        let resume = retained.resolve(books).unwrap();
        assert_eq!(
            resume,
            ResumePoint::Files {
                selected: Some(FileId::new(books.len()))
            }
        );
        let (mut app, _) =
            App::from_resume_with_catalog(books.len(), 1, None, retained.preferences(), resume)
                .unwrap();
        assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
        assert_eq!(app.view(), AppView::Image(ImageId::new(0)));
    }
}

#[test]
fn matched_reader_preserves_preferences_progress_origin_and_navigation() {
    let original = [file("Alpha.epub", 100)];
    let preferences = AppPreferences::from_packed(0x0102_0101).unwrap();
    let retained = SavedResume::capture(reader(0), preferences, &original).unwrap();
    let retained = SavedResume::decode(&retained.encode()).unwrap();
    assert_eq!(retained.preferences(), preferences);
    let (mut app, effect) = App::from_resume(
        1,
        retained.preferences(),
        retained.resolve(&original).unwrap(),
    )
    .unwrap();
    assert_eq!(
        effect,
        AppEffect::LoadChapter {
            book: BookId::new(0),
            spine_index: 2,
            target: PageTarget::Index(3)
        }
    );
    app.chapter_loaded(5, 10).unwrap();
    app.input(AppInput::Move(Direction::Right));
    let ResumePoint::Reader {
        page_index, origin, ..
    } = app.resume_point()
    else {
        panic!("expected reader")
    };
    assert_eq!(page_index, 4);
    assert_eq!(origin, BookOrigin::Files);
    assert_eq!(app.preferences(), preferences);
}

#[test]
fn non_book_views_and_empty_selections_round_trip() {
    let preferences = AppPreferences::default();
    for resume in [
        ResumePoint::Home {
            selected: HomeItem::Settings,
        },
        ResumePoint::Books { selected: None },
        ResumePoint::Files { selected: None },
        ResumePoint::Settings {
            selected: SettingsItem::Size,
            draft: preferences,
        },
        ResumePoint::Image {
            image: ImageId::new(2),
        },
    ] {
        assert_eq!(saved(resume, &[]).resolve(&[]), Ok(resume));
    }
}

#[test]
fn lookup_failure_does_not_discard_valid_preferences() {
    let books = [file("Alpha.epub", 100)];
    let retained = saved(reader(0), &books);
    assert_eq!(retained.resolve(&[]), Err(ResumeError::Missing));
    assert_eq!(retained.preferences(), AppPreferences::default());
}

#[test]
fn legacy_index_records_are_rejected_even_with_a_valid_checksum() {
    for kind in [
        HOME_KIND,
        BOOKS_KIND,
        FILES_KIND,
        SETTINGS_KIND,
        READER_KIND,
        IMAGE_KIND,
    ] {
        let mut legacy = [
            LEGACY_MAGIC,
            kind,
            0,
            0,
            0,
            0,
            AppPreferences::default().packed(),
            0,
        ];
        legacy[7] = checksum(&legacy);
        assert_eq!(
            SavedResume::decode(&legacy),
            Err(ResumeError::LegacyInvalid)
        );
        let mut expanded = [0; RESUME_WORDS];
        expanded[..legacy.len()].copy_from_slice(&legacy);
        assert_eq!(
            SavedResume::decode(&expanded),
            Err(ResumeError::LegacyInvalid)
        );
    }
}

#[test]
fn truncated_unknown_and_corrupt_records_are_rejected() {
    let books = [file("Alpha.epub", 100)];
    let words = saved(reader(0), &books).encode();
    for length in 0..RESUME_WORDS {
        assert_eq!(
            SavedResume::decode(&words[..length]),
            Err(ResumeError::InvalidRecord)
        );
    }
    let mut extended = Vec::from(words);
    extended.push(0);
    assert_eq!(
        SavedResume::decode(&extended),
        Err(ResumeError::InvalidRecord)
    );
    assert_eq!(
        SavedResume::decode(&[0; RESUME_WORDS]),
        Err(ResumeError::InvalidRecord)
    );
    for index in 0..RESUME_WORDS {
        let mut corrupt = words;
        corrupt[index] ^= 1;
        assert_eq!(
            SavedResume::decode(&corrupt),
            Err(ResumeError::InvalidRecord)
        );
    }
}

#[test]
fn valid_checksums_do_not_bypass_record_parsing() {
    let books = [file("Alpha.epub", 100)];
    let words = saved(reader(0), &books).encode();
    for (index, value) in [
        (0, 0x4257_5235),
        (1, 99),
        (2, 1),
        (3, u32::MAX),
        (4, u32::MAX),
        (5, 99),
        (6, u32::MAX),
        (8, 0),
        (8, MAX_BOOK_NAME_BYTES as u32 + 1),
        (10, u32::MAX),
        (RESUME_WORDS - 1, 1),
    ] {
        let mut corrupt = words;
        corrupt[index] = value;
        corrupt[CHECKSUM_WORD] = checksum(&corrupt);
        assert_eq!(
            SavedResume::decode(&corrupt),
            Err(ResumeError::InvalidRecord),
            "word {index}"
        );
    }
}

#[test]
fn capture_rejects_unrepresentable_positions_instead_of_saturating() {
    let books = [file("Alpha.epub", 100)];
    for (spine_index, page_index) in [
        (u32::MAX as usize, 0),
        (0, u32::MAX as usize),
        (0, usize::MAX),
    ] {
        let resume = ResumePoint::Reader {
            book: BookId::new(0),
            spine_index,
            page_index,
            origin: BookOrigin::Books,
        };
        assert_eq!(
            SavedResume::capture(resume, AppPreferences::default(), &books),
            Err(ResumeError::InvalidRecord)
        );
    }
}

#[test]
fn rtc_capacity_is_fixed_and_does_not_depend_on_catalog_size() {
    assert_eq!(core::mem::size_of::<[u32; RESUME_WORDS]>(), 296);
    assert_eq!(core::mem::size_of::<BookIdentity>(), 264);
    let books = [file("Alpha.epub", 100)];
    let words = saved(reader(0), &books).encode();
    let expanded = [books[0]; 16];
    assert_eq!(words.len(), RESUME_WORDS);
    assert_eq!(
        SavedResume::decode(&words).unwrap().resolve(&expanded),
        Err(ResumeError::Ambiguous)
    );
}
