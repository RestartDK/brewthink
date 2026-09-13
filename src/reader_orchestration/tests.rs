extern crate std;

use super::*;
use crate::app::{AppPreferences, AppView, BookOrigin, Direction, HomeItem};
use std::vec::Vec;

#[test]
fn default_images_match_the_const_empty_catalog() {
    const EMPTY: ReaderImages<4> = ReaderImages::empty();
    let images = ReaderImages::<4>::default();
    assert_eq!(images.length, EMPTY.length);
    assert_eq!(images.files, EMPTY.files);
    let zero = ReaderImages::<0>::default();
    assert_eq!(zero.length, 0);
    assert!(zero.files.is_empty());
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IoError {
    Unreadable,
    Corrupt,
    Layout,
    Display,
}

#[derive(Clone, Copy)]
enum RenderBehavior {
    Ready,
    FailOnce(IoError),
    FailAlways(IoError),
    CoverAbsentForever,
}

struct FakeIo {
    chapter: Result<ChapterPages, IoError>,
    cover: bool,
    cover_error: Option<IoError>,
    cover_diagnostics: Vec<IoError>,
    refreshes: usize,
    rendering: RenderBehavior,
    views: Vec<AppView>,
    chapters: Vec<(BookId, usize)>,
    chapter_ready: bool,
}

impl FakeIo {
    fn ready() -> Self {
        Self {
            chapter: Ok(ChapterPages {
                spine_count: 2,
                page_count: 3,
            }),
            cover: false,
            cover_error: None,
            cover_diagnostics: Vec::new(),
            refreshes: 0,
            rendering: RenderBehavior::Ready,
            views: Vec::new(),
            chapters: Vec::new(),
            chapter_ready: false,
        }
    }
}

impl ReaderIo for FakeIo {
    type Error = IoError;

    fn load_chapter(
        &mut self,
        book: BookId,
        spine_index: usize,
        _: ReaderPreferences,
    ) -> Result<ChapterPages, Self::Error> {
        self.chapters.push((book, spine_index));
        self.chapter_ready = self.chapter.is_ok();
        self.chapter
    }

    fn render(&mut self, app: &App) -> Result<Rendered, Self::Error> {
        self.views.push(app.view());
        match app.view() {
            AppView::Sleeping { .. } => self.chapter_ready = false,
            AppView::Reader(_) | AppView::ReaderDrawer(_) => {
                assert!(self.chapter_ready, "rendered overwritten chapter bytes");
            }
            _ => {}
        }
        if matches!(self.rendering, RenderBehavior::CoverAbsentForever) {
            return Ok(Rendered::CoverAbsent);
        }
        let mut refresh = || {
            self.refreshes += 1;
            match self.rendering {
                RenderBehavior::FailOnce(error) => {
                    self.rendering = RenderBehavior::Ready;
                    Err(error)
                }
                RenderBehavior::FailAlways(error) => Err(error),
                _ => Ok(()),
            }
        };
        if matches!(app.view(), AppView::BookCover { .. }) {
            render_cover(
                self.cover_error.map_or(Ok(self.cover), Err),
                |error| self.cover_diagnostics.push(*error),
                refresh,
            )
        } else {
            refresh().map(|_| Rendered::Frame)
        }
    }
}

fn input(
    app: &mut App,
    io: &mut FakeIo,
    input: AppInput,
) -> Result<Option<ResumePoint>, OperationFailure<IoError>> {
    let effect = app.input(input);
    run_effect(effect, app, io)
}

#[test]
fn chapter_read_failure_recovers_then_another_book_opens() {
    for error in [IoError::Unreadable, IoError::Corrupt, IoError::Layout] {
        let mut app = App::new(2);
        let mut io = FakeIo::ready();
        input(&mut app, &mut io, AppInput::Confirm).unwrap();
        io.chapter = Err(error);
        assert_eq!(
            input(&mut app, &mut io, AppInput::Confirm),
            Err(OperationFailure {
                cause: Failure::Chapter(error),
                recovery: None
            })
        );
        assert!(matches!(io.views.last(), Some(AppView::Error { .. })));
        assert_eq!(io.chapters.len(), 1);
        input(&mut app, &mut io, AppInput::Back).unwrap();
        input(&mut app, &mut io, AppInput::Move(Direction::Right)).unwrap();
        io.chapter = FakeIo::ready().chapter;
        input(&mut app, &mut io, AppInput::Confirm).unwrap();
        let AppView::Reader(session) = app.view() else {
            panic!("not reading")
        };
        assert_eq!(session.location().book(), BookId::new(1));
    }
}

#[test]
fn rejected_chapter_metadata_leaves_loading_and_can_retry() {
    for (chapter, expected) in [
        (
            ChapterPages {
                spine_count: 0,
                page_count: 3,
            },
            AppStateError::SpineOutOfBounds,
        ),
        (
            ChapterPages {
                spine_count: 2,
                page_count: 0,
            },
            AppStateError::EmptyChapter,
        ),
    ] {
        let mut app = App::new(1);
        let mut io = FakeIo::ready();
        input(&mut app, &mut io, AppInput::Confirm).unwrap();
        io.chapter = Ok(chapter);
        assert_eq!(
            input(&mut app, &mut io, AppInput::Confirm)
                .unwrap_err()
                .cause,
            Failure::State(expected)
        );
        assert!(matches!(app.view(), AppView::Error { .. }));
        input(&mut app, &mut io, AppInput::Back).unwrap();
        io.chapter = FakeIo::ready().chapter;
        input(&mut app, &mut io, AppInput::Confirm).unwrap();
        assert!(matches!(app.view(), AppView::Reader(_)));
    }
}

#[test]
fn resumed_chapter_failure_returns_to_the_saved_origin() {
    let resume = ResumePoint::Reader {
        book: BookId::new(0),
        spine_index: 1,
        page_index: 2,
        origin: BookOrigin::Files,
    };
    let (mut app, effect) = App::from_resume(1, AppPreferences::default(), resume).unwrap();
    let mut io = FakeIo::ready();
    io.chapter = Err(IoError::Unreadable);
    run_effect(effect, &mut app, &mut io).unwrap_err();
    input(&mut app, &mut io, AppInput::Back).unwrap();
    assert!(matches!(app.view(), AppView::Files(_)));
    io.chapter = FakeIo::ready().chapter;
    input(&mut app, &mut io, AppInput::Confirm).unwrap();
    assert!(matches!(app.view(), AppView::Reader(_)));
}

#[test]
fn chapter_boundary_failure_keeps_navigation_usable() {
    let mut app = App::new(1);
    let mut io = FakeIo::ready();
    input(&mut app, &mut io, AppInput::Confirm).unwrap();
    input(&mut app, &mut io, AppInput::Confirm).unwrap();
    for _ in 0..2 {
        input(&mut app, &mut io, AppInput::Move(Direction::Right)).unwrap();
    }
    io.chapter = Err(IoError::Unreadable);
    input(&mut app, &mut io, AppInput::Move(Direction::Right)).unwrap_err();
    assert_eq!(io.chapters.last(), Some(&(BookId::new(0), 1)));
    input(&mut app, &mut io, AppInput::Back).unwrap();
    assert_eq!(app.view(), AppView::Library);
}

#[test]
fn existing_cover_waits_for_confirm_but_absent_cover_opens_the_chapter() {
    for cover in [true, false] {
        let mut app = App::new(1);
        let mut io = FakeIo::ready();
        io.cover = cover;
        input(&mut app, &mut io, AppInput::Confirm).unwrap();
        input(&mut app, &mut io, AppInput::Confirm).unwrap();
        if cover {
            assert!(matches!(app.view(), AppView::BookCover { .. }));
            assert!(io.chapters.is_empty());
            input(&mut app, &mut io, AppInput::Confirm).unwrap();
        }
        assert!(matches!(app.view(), AppView::Reader(_)));
        assert_eq!(io.chapters.len(), 1);
    }
}

#[test]
fn cover_display_failure_does_not_masquerade_as_absence() {
    let mut app = App::new(1);
    let mut io = FakeIo::ready();
    input(&mut app, &mut io, AppInput::Confirm).unwrap();
    io.cover = true;
    io.rendering = RenderBehavior::FailOnce(IoError::Display);
    assert_eq!(
        input(&mut app, &mut io, AppInput::Confirm)
            .unwrap_err()
            .cause,
        Failure::Render(IoError::Display)
    );
    assert!(io.chapters.is_empty());
    assert!(matches!(app.view(), AppView::Error { .. }));
    input(&mut app, &mut io, AppInput::Back).unwrap();
    assert_eq!(app.view(), AppView::Library);
}

#[test]
fn unreadable_image_returns_to_files_without_selecting_it() {
    let mut app = App::with_catalog(0, 2, Some(ImageId::new(0)), AppPreferences::default());
    let mut io = FakeIo::ready();
    input(&mut app, &mut io, AppInput::Move(Direction::Down)).unwrap();
    input(&mut app, &mut io, AppInput::Confirm).unwrap();
    input(&mut app, &mut io, AppInput::Move(Direction::Down)).unwrap();
    io.rendering = RenderBehavior::FailOnce(IoError::Unreadable);
    input(&mut app, &mut io, AppInput::Confirm).unwrap_err();
    assert!(matches!(app.view(), AppView::Files(_)));
    assert_eq!(app.selected_sleep_image(), Some(ImageId::new(0)));
    input(&mut app, &mut io, AppInput::Confirm).unwrap();
    assert_eq!(app.view(), AppView::Image(ImageId::new(1)));
    input(&mut app, &mut io, AppInput::Confirm).unwrap();
    assert_eq!(app.selected_sleep_image(), Some(ImageId::new(1)));
}

#[test]
fn failed_sleep_frame_never_enters_sleep_and_can_be_retried() {
    let mut app = App::new(1);
    let mut io = FakeIo::ready();
    io.rendering = RenderBehavior::FailOnce(IoError::Display);
    input(&mut app, &mut io, AppInput::Power).unwrap_err();
    assert!(matches!(app.view(), AppView::Home(_)));
    assert_eq!(
        input(&mut app, &mut io, AppInput::Power),
        Ok(Some(ResumePoint::Home {
            selected: HomeItem::Books
        }))
    );
}

#[test]
fn failing_recovery_render_is_attempted_once_and_both_causes_survive() {
    let resume = ResumePoint::Reader {
        book: BookId::new(0),
        spine_index: 0,
        page_index: 0,
        origin: BookOrigin::Books,
    };
    let (mut app, effect) = App::from_resume(1, AppPreferences::default(), resume).unwrap();
    let mut io = FakeIo::ready();
    io.chapter = Err(IoError::Unreadable);
    io.rendering = RenderBehavior::FailAlways(IoError::Display);
    assert_eq!(
        run_effect(effect, &mut app, &mut io),
        Err(OperationFailure {
            cause: Failure::Chapter(IoError::Unreadable),
            recovery: Some(Failure::Render(IoError::Display)),
        })
    );
    assert_eq!(io.views.len(), 1);
    io.rendering = RenderBehavior::Ready;
    input(&mut app, &mut io, AppInput::Back).unwrap();
    assert_eq!(app.view(), AppView::Library);
}

#[test]
fn invalid_backend_completion_is_bounded() {
    let mut app = App::new(1);
    let mut io = FakeIo::ready();
    io.rendering = RenderBehavior::CoverAbsentForever;
    assert_eq!(
        run_effect(AppEffect::Render, &mut app, &mut io)
            .unwrap_err()
            .cause,
        Failure::EffectLimit
    );
    assert_eq!(io.views.len(), 4);
}

#[test]
fn failed_catalog_startup_can_be_explicitly_skipped_without_blocking_navigation() {
    for stage in [
        StartupStage::Layout,
        StartupStage::Books,
        StartupStage::Images,
    ] {
        let mut startup = Startup::Run(stage);
        startup.complete(Err(IoError::Corrupt)).unwrap_err();
        startup.input(AppInput::Back);
        assert_eq!(startup, stage.next());
        while startup != Startup::Ready {
            startup.complete(Ok::<_, IoError>(())).unwrap();
        }
        let mut app = App::new(0);
        let mut io = FakeIo::ready();
        input(&mut app, &mut io, AppInput::Confirm).unwrap();
        assert_eq!(app.view(), AppView::Library);
        input(&mut app, &mut io, AppInput::Back).unwrap();
        input(&mut app, &mut io, AppInput::Move(Direction::Down)).unwrap();
        input(&mut app, &mut io, AppInput::Confirm).unwrap();
        assert!(matches!(app.view(), AppView::Files(_)));
    }
    for stage in [StartupStage::Card, StartupStage::Display] {
        let mut startup = Startup::AwaitingRetry(stage);
        startup.input(AppInput::Back);
        assert_eq!(startup, Startup::AwaitingRetry(stage));
    }
}

#[test]
fn startup_failures_require_one_explicit_retry_then_reach_a_usable_app() {
    for failed_stage in [
        StartupStage::Card,
        StartupStage::Layout,
        StartupStage::Books,
        StartupStage::Images,
        StartupStage::Display,
    ] {
        let mut startup = Startup::Run(StartupStage::Card);
        while startup != Startup::Run(failed_stage) {
            startup.complete(Ok::<_, IoError>(())).unwrap();
        }
        assert_eq!(
            startup.complete(Err(IoError::Unreadable)),
            Err(IoError::Unreadable)
        );
        assert_eq!(startup, Startup::AwaitingRetry(failed_stage));
        for button in [AppInput::Power, AppInput::Move(Direction::Right)] {
            startup.input(button);
            assert_eq!(startup, Startup::AwaitingRetry(failed_stage));
        }
        startup.input(AppInput::Confirm);
        assert_eq!(startup, Startup::Run(failed_stage));
        while startup != Startup::Ready {
            startup.complete(Ok::<_, IoError>(())).unwrap();
        }
        let mut app = App::new(1);
        let mut io = FakeIo::ready();
        input(&mut app, &mut io, AppInput::Confirm).unwrap();
        input(&mut app, &mut io, AppInput::Confirm).unwrap();
        assert!(matches!(app.view(), AppView::Reader(_)));
    }
}

fn reading_nonzero_position() -> (App, FakeIo, ResumePoint) {
    let resume = ResumePoint::Reader {
        book: BookId::new(1),
        spine_index: 1,
        page_index: 2,
        origin: BookOrigin::Files,
    };
    let (mut app, effect) = App::from_resume(2, AppPreferences::default(), resume).unwrap();
    let mut io = FakeIo::ready();
    run_effect(effect, &mut app, &mut io).unwrap();
    (app, io, resume)
}

#[test]
fn failed_sleep_reloads_nonzero_position_before_retry_and_serialized_wake() {
    use crate::storage::{BookFile, BookFileName, book_resume::SavedResume};

    let (mut app, mut io, expected) = reading_nonzero_position();
    io.rendering = RenderBehavior::FailOnce(IoError::Display);
    assert_eq!(
        input(&mut app, &mut io, AppInput::Power),
        Err(OperationFailure {
            cause: Failure::Render(IoError::Display),
            recovery: None,
        })
    );
    assert_eq!(io.chapters, [(BookId::new(1), 1), (BookId::new(1), 1)]);
    assert!(matches!(app.view(), AppView::Reader(_)));
    let resume = input(&mut app, &mut io, AppInput::Power).unwrap().unwrap();
    assert_eq!(resume, expected);
    let books = [
        Some(BookFile::new(
            BookFileName::try_from("FIRST.EPUB").unwrap(),
            100,
        )),
        Some(BookFile::new(
            BookFileName::try_from("SECOND.EPUB").unwrap(),
            200,
        )),
    ];
    let words = SavedResume::capture(resume, app.preferences(), &books)
        .unwrap()
        .encode();
    let saved = SavedResume::decode(&words).unwrap();
    let restored = saved.resolve(&books).unwrap();
    assert_eq!(restored, expected);
    let (mut awake, effect) = App::from_resume(2, saved.preferences(), restored).unwrap();
    run_effect(effect, &mut awake, &mut io).unwrap();
    let AppView::Reader(session) = awake.view() else {
        panic!("not reading after wake")
    };
    assert_eq!(session.location().book(), BookId::new(1));
    assert_eq!(session.location().spine_index(), 1);
    assert_eq!(session.location().page_index(), 2);
}

#[test]
fn failed_sleep_and_failed_recovery_reload_preserve_both_errors_without_stale_render() {
    let (mut app, mut io, _) = reading_nonzero_position();
    io.chapter = Err(IoError::Unreadable);
    io.rendering = RenderBehavior::FailOnce(IoError::Display);
    let views_before = io.views.len();
    assert_eq!(
        input(&mut app, &mut io, AppInput::Power),
        Err(OperationFailure {
            cause: Failure::Render(IoError::Display),
            recovery: Some(Failure::Chapter(IoError::Unreadable)),
        })
    );
    assert_eq!(io.views.len(), views_before + 1);
    assert_eq!(io.chapters.len(), 2);
    assert!(matches!(app.view(), AppView::Error { .. }));
    input(&mut app, &mut io, AppInput::Back).unwrap();
    assert!(matches!(app.view(), AppView::Files(_)));
    io.chapter = FakeIo::ready().chapter;
    input(&mut app, &mut io, AppInput::Confirm).unwrap();
    assert!(matches!(app.view(), AppView::Reader(_)));
}

#[test]
fn failed_sleep_and_failed_recovery_refresh_preserve_both_errors() {
    let (mut app, mut io, _) = reading_nonzero_position();
    io.rendering = RenderBehavior::FailAlways(IoError::Display);
    assert_eq!(
        input(&mut app, &mut io, AppInput::Power),
        Err(OperationFailure {
            cause: Failure::Render(IoError::Display),
            recovery: Some(Failure::Render(IoError::Display)),
        })
    );
    assert_eq!(io.chapters.len(), 2);
    assert!(matches!(app.view(), AppView::Error { .. }));
}

#[test]
fn permanent_cover_decode_failure_reports_diagnostic_and_opens_readable_chapter() {
    for error in [IoError::Corrupt, IoError::Layout, IoError::Unreadable] {
        let mut app = App::new(1);
        let mut io = FakeIo::ready();
        io.cover_error = Some(error);
        input(&mut app, &mut io, AppInput::Confirm).unwrap();
        let refreshes = io.refreshes;
        assert_eq!(input(&mut app, &mut io, AppInput::Confirm), Ok(None));
        assert!(matches!(app.view(), AppView::Reader(_)));
        assert_eq!(io.cover_diagnostics, [error]);
        assert_eq!(io.chapters, [(BookId::new(0), 0)]);
        assert_eq!(io.refreshes, refreshes + 1);
        assert_eq!(io.cover_error, Some(error));
    }
}
