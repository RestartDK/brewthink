use super::*;

fn book_progress(spine_index: usize, page_index: usize, page_count: usize) -> BookProgress {
    BookProgress::new(
        spine_index,
        page_index,
        page_count,
        AppPreferences::default().reader(),
    )
    .unwrap()
}

#[test]
fn opening_a_book_reads_stored_progress_before_showing_the_cover() {
    let mut app = App::new(2);
    app.input(AppInput::Confirm);
    assert_eq!(app.view(), AppView::Library);
    assert_eq!(
        app.input(AppInput::Confirm),
        AppEffect::LoadProgress {
            book: BookId::new(0),
            origin: BookOrigin::Books,
        }
    );
    assert_eq!(
        app.progress_loaded(BookId::new(0), BookOrigin::Books, None),
        AppEffect::Render
    );
    assert!(matches!(app.view(), AppView::BookCover { .. }));
}

#[test]
fn stored_progress_reopens_the_page_and_reflows_when_typography_changed() {
    let mut app = App::new(1);
    app.input(AppInput::Confirm);
    assert_eq!(
        app.progress_loaded(
            BookId::new(0),
            BookOrigin::Books,
            Some(book_progress(4, 2, 10))
        ),
        AppEffect::LoadChapter {
            book: BookId::new(0),
            spine_index: 4,
            target: PageTarget::Index(2),
        }
    );

    let mut other = App::with_preferences(
        1,
        AppPreferences::new(
            ReaderPreferences::new(
                ReaderFont::Compact,
                ReaderFontSize::Large,
                ReaderSpacing::Relaxed,
            ),
            SleepScreenMode::Automatic,
        ),
    );
    other.input(AppInput::Confirm);
    assert_eq!(
        other.progress_loaded(
            BookId::new(0),
            BookOrigin::Books,
            Some(book_progress(4, 2, 10))
        ),
        AppEffect::LoadChapter {
            book: BookId::new(0),
            spine_index: 4,
            target: PageTarget::Progress {
                page_index: 2,
                page_count: 10,
            },
        }
    );
}

#[test]
fn book_progress_tracks_the_open_page_only_while_reading() {
    let mut app = App::new(1);
    assert_eq!(app.book_progress(), None);
    app.input(AppInput::Confirm);
    app.input_without_stored_progress(AppInput::Confirm);
    app.input(AppInput::Confirm);
    app.chapter_loaded(3, 8).unwrap();
    assert_eq!(
        app.book_progress(),
        Some((BookId::new(0), book_progress(0, 0, 8)))
    );
    app.input(AppInput::Move(Direction::Right));
    assert_eq!(
        app.book_progress(),
        Some((BookId::new(0), book_progress(0, 1, 8)))
    );
    app.input(AppInput::Back);
    assert_eq!(app.book_progress(), None);
}

#[test]
fn progress_bounds_reject_values_that_cannot_be_stored() {
    let preferences = AppPreferences::default().reader();
    assert!(BookProgress::new(0, 0, 1, preferences).is_some());
    assert!(BookProgress::new(0, 0, 0, preferences).is_none());
    assert!(BookProgress::new(0, 1, 1, preferences).is_none());
    assert!(BookProgress::new(usize::MAX, 0, 1, preferences).is_none());
    assert!(BookProgress::new(0, 0, usize::MAX, preferences).is_none());
}
