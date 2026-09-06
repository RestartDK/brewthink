use super::*;

fn reading(pages: usize, chapters: usize) -> App {
    let mut app = App::new(1);
    app.input(AppInput::Confirm);
    app.input(AppInput::Confirm);
    app.chapter_loaded(chapters, pages).unwrap();
    app
}

fn drawer(app: App) -> ReaderDrawer {
    let AppView::ReaderDrawer(drawer) = app.view() else {
        panic!("expected reader drawer")
    };
    drawer
}

#[test]
fn confirm_opens_controls_without_turning_the_page_and_back_cancels() {
    let mut app = reading(100, 3);
    app.input(AppInput::Move(Direction::Right));
    let original = app.resume_point();
    assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
    assert_eq!(drawer(app).page(), 1);
    app.input(AppInput::Move(Direction::Right));
    assert_eq!(drawer(app).page(), 6);
    assert_eq!(app.resume_point(), original);
    app.input(AppInput::Back);
    assert_eq!(app.resume_point(), original);
    assert!(matches!(app.view(), AppView::Reader(_)));
}

#[test]
fn page_slider_is_bounded_and_jumps_only_on_confirm() {
    for pages in [1, 2, 19, 20, 21, 100, 1000] {
        let mut app = reading(pages, 1);
        app.input(AppInput::Confirm);
        for _ in 0..30 {
            app.input(AppInput::Move(Direction::Right));
        }
        assert_eq!(drawer(app).page(), pages - 1);
        assert_eq!(app.input(AppInput::Move(Direction::Right)), AppEffect::None);
        assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
        let AppView::Reader(session) = app.view() else {
            panic!("reader expected")
        };
        assert_eq!(session.location().page_index(), pages - 1);
        app.input(AppInput::Confirm);
        for _ in 0..30 {
            app.input(AppInput::Move(Direction::Left));
        }
        assert_eq!(drawer(app).page(), 0);
        assert_eq!(app.input(AppInput::Move(Direction::Left)), AppEffect::None);
    }
}

#[test]
fn chapter_picker_is_bounded_and_defers_io_until_confirm() {
    let mut app = reading(8, 3);
    app.input(AppInput::Confirm);
    app.input(AppInput::Move(Direction::Down));
    assert_eq!(drawer(app).selected(), ReaderControl::Chapter);
    for _ in 0..4 {
        app.input(AppInput::Move(Direction::Right));
    }
    assert_eq!(drawer(app).chapter(), 2);
    assert_eq!(app.input(AppInput::Move(Direction::Right)), AppEffect::None);
    assert_eq!(
        app.input(AppInput::Confirm),
        AppEffect::LoadChapter {
            book: BookId::new(0),
            spine_index: 2,
            target: PageTarget::First,
        }
    );
    app.chapter_loaded(3, 12).unwrap();
    let AppView::Reader(session) = app.view() else {
        panic!("reader expected")
    };
    assert_eq!(session.location().spine_index(), 2);
    assert_eq!(session.location().page_index(), 0);
}

#[test]
fn typography_applies_with_progress_reflow_and_preserves_sleep_preferences() {
    let mut app = reading(100, 3);
    for _ in 0..50 {
        app.input(AppInput::Move(Direction::Right));
    }
    let original = app.reader_preferences();
    app.input(AppInput::Confirm);
    for _ in 0..3 {
        app.input(AppInput::Move(Direction::Down));
    }
    app.input(AppInput::Move(Direction::Right));
    assert_ne!(drawer(app).preferences(), original);
    assert_eq!(app.reader_preferences(), original);
    assert_eq!(
        app.input(AppInput::Confirm),
        AppEffect::LoadChapter {
            book: BookId::new(0),
            spine_index: 0,
            target: PageTarget::Progress {
                page_index: 50,
                page_count: 100
            },
        }
    );
    assert_eq!(app.reader_preferences().size(), ReaderFontSize::Large);
    assert_eq!(app.preferences().sleep_screen(), SleepScreenMode::Automatic);
    app.chapter_loaded(3, 200).unwrap();
    let AppView::Reader(session) = app.view() else {
        panic!("reader expected")
    };
    assert_eq!(session.location().page_index(), 100);
}

#[test]
fn cancelling_and_sleeping_discard_draft_typography_and_restore_reader() {
    for input in [AppInput::Back, AppInput::Power] {
        let mut app = reading(8, 3);
        let original = app.resume_point();
        let preferences = app.preferences();
        app.input(AppInput::Confirm);
        for _ in 0..2 {
            app.input(AppInput::Move(Direction::Down));
        }
        app.input(AppInput::Move(Direction::Right));
        app.input(input);
        assert_eq!(app.preferences(), preferences);
        assert_eq!(app.resume_point(), original);
        if input == AppInput::Power {
            app.sleep_frame_ready().unwrap();
            let effect = app.wake();
            if matches!(effect, AppEffect::LoadChapter { .. }) {
                app.chapter_loaded(3, 8).unwrap();
            }
        }
        assert!(matches!(app.view(), AppView::Reader(_)));
    }
}

#[test]
fn hidden_status_does_not_refresh_the_reading_or_sleeping_frame() {
    use crate::input::UsbState;
    let mut app = reading(8, 3);
    assert_eq!(
        app.set_battery(BatteryStatus::from_percent(70, UsbState::Disconnected)),
        AppEffect::None
    );
    app.input(AppInput::Confirm);
    assert_eq!(
        app.set_battery(BatteryStatus::from_percent(50, UsbState::Disconnected)),
        AppEffect::Render
    );
    app.input(AppInput::Power);
    assert_eq!(
        app.set_battery(BatteryStatus::from_percent(30, UsbState::Disconnected)),
        AppEffect::None
    );
}
