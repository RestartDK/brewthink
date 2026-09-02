const BOOKS_PER_SHELF_PAGE: usize = 4;
const SHELF_COLUMNS: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BookId(usize);

impl BookId {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionOutOfBounds;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibraryState {
    book_count: usize,
    selected: Option<BookId>,
}

impl LibraryState {
    pub const fn new(book_count: usize) -> Self {
        Self {
            book_count,
            selected: if book_count == 0 {
                None
            } else {
                Some(BookId(0))
            },
        }
    }

    pub const fn with_selected(
        book_count: usize,
        selected: usize,
    ) -> Result<Self, SelectionOutOfBounds> {
        if selected >= book_count {
            return Err(SelectionOutOfBounds);
        }
        Ok(Self {
            book_count,
            selected: Some(BookId(selected)),
        })
    }

    pub const fn book_count(self) -> usize {
        self.book_count
    }

    pub const fn selected(self) -> Option<BookId> {
        self.selected
    }

    pub const fn page(self) -> usize {
        match self.selected {
            Some(selected) => selected.0 / BOOKS_PER_SHELF_PAGE,
            None => 0,
        }
    }

    pub const fn page_count(self) -> usize {
        self.book_count.div_ceil(BOOKS_PER_SHELF_PAGE)
    }

    pub fn visible_range(self) -> core::ops::Range<usize> {
        let start = self.page() * BOOKS_PER_SHELF_PAGE;
        let end = (start + BOOKS_PER_SHELF_PAGE).min(self.book_count);
        start..end
    }

    pub fn move_selection(&mut self, direction: Direction) -> bool {
        let Some(selected) = self.selected else {
            return false;
        };
        let next = match direction {
            Direction::Left if selected.0 % SHELF_COLUMNS == 1 => selected.0 - 1,
            Direction::Right
                if selected.0 % SHELF_COLUMNS == 0 && selected.0 + 1 < self.book_count =>
            {
                selected.0 + 1
            }
            Direction::Up => selected.0.saturating_sub(SHELF_COLUMNS),
            Direction::Down => (selected.0 + SHELF_COLUMNS).min(self.book_count - 1),
            Direction::Left | Direction::Right => selected.0,
        };
        if next == selected.0 {
            return false;
        }
        self.selected = Some(BookId(next));
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppInput {
    Move(Direction),
    Confirm,
    Back,
    Power,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageTarget {
    First,
    Last,
    Index(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadingLocation {
    book: BookId,
    spine_index: usize,
    spine_count: usize,
    page_index: usize,
    page_count: usize,
}

impl ReadingLocation {
    pub const fn book(self) -> BookId {
        self.book
    }

    pub const fn spine_index(self) -> usize {
        self.spine_index
    }

    pub const fn spine_count(self) -> usize {
        self.spine_count
    }

    pub const fn page_index(self) -> usize {
        self.page_index
    }

    pub const fn page_count(self) -> usize {
        self.page_count
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumePoint {
    Library {
        selected: Option<BookId>,
    },
    Reader {
        book: BookId,
        spine_index: usize,
        page_index: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppView {
    Library,
    Loading,
    Reader(ReadingLocation),
    Error { book: BookId },
    Sleeping { resume: ResumePoint },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppEffect {
    None,
    RenderLibrary,
    LoadChapter {
        book: BookId,
        spine_index: usize,
        target: PageTarget,
    },
    RenderReader(ReadingLocation),
    RenderError {
        book: BookId,
    },
    RenderSleep {
        resume: ResumePoint,
    },
    EnterDeepSleep {
        resume: ResumePoint,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppStateError {
    BookOutOfBounds,
    SpineOutOfBounds,
    EmptyChapter,
    UnexpectedChapter,
    NotPreparingSleep,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingChapter {
    book: BookId,
    spine_index: usize,
    target: PageTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReadingCheckpoint {
    book: BookId,
    spine_index: usize,
    page_index: usize,
}

impl From<ReadingLocation> for ReadingCheckpoint {
    fn from(location: ReadingLocation) -> Self {
        Self {
            book: location.book,
            spine_index: location.spine_index,
            page_index: location.page_index,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct App {
    library: LibraryState,
    view: AppView,
    pending: Option<PendingChapter>,
    reading_checkpoint: Option<ReadingCheckpoint>,
}

impl App {
    pub const fn new(book_count: usize) -> Self {
        Self {
            library: LibraryState::new(book_count),
            view: AppView::Library,
            pending: None,
            reading_checkpoint: None,
        }
    }

    pub fn from_resume(
        book_count: usize,
        resume: ResumePoint,
    ) -> Result<(Self, AppEffect), AppStateError> {
        let mut app = Self::new(book_count);
        let effect = match resume {
            ResumePoint::Library { selected: None } if book_count == 0 => AppEffect::RenderLibrary,
            ResumePoint::Library {
                selected: Some(selected),
            } => {
                app.library = LibraryState::with_selected(book_count, selected.index())
                    .map_err(|_| AppStateError::BookOutOfBounds)?;
                AppEffect::RenderLibrary
            }
            ResumePoint::Library { selected: None } => AppEffect::RenderLibrary,
            ResumePoint::Reader {
                book,
                spine_index,
                page_index,
            } => {
                app.validate_book(book)?;
                app.reading_checkpoint = Some(ReadingCheckpoint {
                    book,
                    spine_index,
                    page_index,
                });
                app.request_chapter(book, spine_index, PageTarget::Index(page_index))
            }
        };
        Ok((app, effect))
    }

    pub const fn library(self) -> LibraryState {
        self.library
    }

    pub const fn view(self) -> AppView {
        self.view
    }

    pub fn resume_point(self) -> ResumePoint {
        match self.view {
            AppView::Reader(location) => ResumePoint::Reader {
                book: location.book,
                spine_index: location.spine_index,
                page_index: location.page_index,
            },
            AppView::Sleeping { resume } => resume,
            AppView::Loading => ResumePoint::Library {
                selected: self
                    .pending
                    .map(|pending| pending.book)
                    .or(self.library.selected),
            },
            AppView::Error { book } => ResumePoint::Library {
                selected: Some(book),
            },
            AppView::Library => ResumePoint::Library {
                selected: self.library.selected,
            },
        }
    }

    pub fn input(&mut self, input: AppInput) -> AppEffect {
        if input == AppInput::Power && !matches!(self.view, AppView::Sleeping { .. }) {
            let resume = self.resume_point();
            self.view = AppView::Sleeping { resume };
            self.pending = None;
            return AppEffect::RenderSleep { resume };
        }

        match (self.view, input) {
            (AppView::Library, AppInput::Move(direction)) => {
                if self.library.move_selection(direction) {
                    AppEffect::RenderLibrary
                } else {
                    AppEffect::None
                }
            }
            (AppView::Library, AppInput::Confirm) => {
                let Some(book) = self.library.selected else {
                    return AppEffect::None;
                };
                match self
                    .reading_checkpoint
                    .filter(|checkpoint| checkpoint.book == book)
                {
                    Some(checkpoint) => self.request_chapter(
                        book,
                        checkpoint.spine_index,
                        PageTarget::Index(checkpoint.page_index),
                    ),
                    None => self.request_chapter(book, 0, PageTarget::First),
                }
            }
            (AppView::Reader(location), AppInput::Move(Direction::Right | Direction::Down))
            | (AppView::Reader(location), AppInput::Confirm) => self.next_page(location),
            (AppView::Reader(location), AppInput::Move(Direction::Left | Direction::Up)) => {
                self.previous_page(location)
            }
            (AppView::Reader(_), AppInput::Back)
            | (AppView::Loading, AppInput::Back)
            | (AppView::Error { .. }, AppInput::Back) => {
                self.view = AppView::Library;
                self.pending = None;
                AppEffect::RenderLibrary
            }
            _ => AppEffect::None,
        }
    }

    pub fn chapter_loaded(
        &mut self,
        spine_count: usize,
        page_count: usize,
    ) -> Result<AppEffect, AppStateError> {
        let pending = self
            .pending
            .take()
            .ok_or(AppStateError::UnexpectedChapter)?;
        if spine_count == 0 || pending.spine_index >= spine_count {
            return Err(AppStateError::SpineOutOfBounds);
        }
        if page_count == 0 {
            return Err(AppStateError::EmptyChapter);
        }
        let page_index = match pending.target {
            PageTarget::First => 0,
            PageTarget::Last => page_count - 1,
            PageTarget::Index(index) => index.min(page_count - 1),
        };
        let location = ReadingLocation {
            book: pending.book,
            spine_index: pending.spine_index,
            spine_count,
            page_index,
            page_count,
        };
        self.view = AppView::Reader(location);
        self.reading_checkpoint = Some(location.into());
        Ok(AppEffect::RenderReader(location))
    }

    pub fn chapter_failed(&mut self) -> Result<AppEffect, AppStateError> {
        let pending = self
            .pending
            .take()
            .ok_or(AppStateError::UnexpectedChapter)?;
        self.view = AppView::Error { book: pending.book };
        Ok(AppEffect::RenderError { book: pending.book })
    }

    pub fn sleep_frame_ready(&self) -> Result<AppEffect, AppStateError> {
        match self.view {
            AppView::Sleeping { resume } => Ok(AppEffect::EnterDeepSleep { resume }),
            _ => Err(AppStateError::NotPreparingSleep),
        }
    }

    pub fn wake(&mut self) -> AppEffect {
        let AppView::Sleeping { resume } = self.view else {
            return AppEffect::None;
        };
        match resume {
            ResumePoint::Library { .. } => {
                self.view = AppView::Library;
                AppEffect::RenderLibrary
            }
            ResumePoint::Reader {
                book,
                spine_index,
                page_index,
            } => self.request_chapter(book, spine_index, PageTarget::Index(page_index)),
        }
    }

    fn next_page(&mut self, location: ReadingLocation) -> AppEffect {
        if location.page_index + 1 < location.page_count {
            let next = ReadingLocation {
                page_index: location.page_index + 1,
                ..location
            };
            self.view = AppView::Reader(next);
            self.reading_checkpoint = Some(next.into());
            AppEffect::RenderReader(next)
        } else if location.spine_index + 1 < location.spine_count {
            self.request_chapter(location.book, location.spine_index + 1, PageTarget::First)
        } else {
            AppEffect::None
        }
    }

    fn previous_page(&mut self, location: ReadingLocation) -> AppEffect {
        if location.page_index > 0 {
            let previous = ReadingLocation {
                page_index: location.page_index - 1,
                ..location
            };
            self.view = AppView::Reader(previous);
            self.reading_checkpoint = Some(previous.into());
            AppEffect::RenderReader(previous)
        } else if location.spine_index > 0 {
            self.request_chapter(location.book, location.spine_index - 1, PageTarget::Last)
        } else {
            AppEffect::None
        }
    }

    fn request_chapter(
        &mut self,
        book: BookId,
        spine_index: usize,
        target: PageTarget,
    ) -> AppEffect {
        self.view = AppView::Loading;
        self.pending = Some(PendingChapter {
            book,
            spine_index,
            target,
        });
        AppEffect::LoadChapter {
            book,
            spine_index,
            target,
        }
    }

    fn validate_book(&self, book: BookId) -> Result<(), AppStateError> {
        if book.index() < self.library.book_count {
            Ok(())
        } else {
            Err(AppStateError::BookOutOfBounds)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        App, AppEffect, AppInput, AppView, BookId, Direction, LibraryState, PageTarget, ResumePoint,
    };

    #[test]
    fn an_empty_library_has_no_selection_or_pages() {
        let mut state = LibraryState::new(0);

        assert_eq!(state.selected(), None);
        assert_eq!(state.page_count(), 0);
        assert_eq!(state.visible_range(), 0..0);
        assert!(!state.move_selection(Direction::Right));
    }

    #[test]
    fn directional_navigation_stays_within_the_catalog() {
        let mut state = LibraryState::new(3);

        assert!(!state.move_selection(Direction::Left));
        assert!(state.move_selection(Direction::Right));
        assert_eq!(state.selected(), Some(BookId(1)));
        assert!(!state.move_selection(Direction::Right));
        assert!(state.move_selection(Direction::Down));
        assert_eq!(state.selected(), Some(BookId(2)));
        assert!(!state.move_selection(Direction::Down));
    }

    #[test]
    fn an_explicit_selection_must_belong_to_the_catalog() {
        assert!(LibraryState::with_selected(2, 2).is_err());
        assert_eq!(
            LibraryState::with_selected(2, 1).unwrap().selected(),
            Some(BookId(1))
        );
    }

    #[test]
    fn moving_beyond_four_books_advances_the_visible_page() {
        let mut state = LibraryState::new(7);

        assert!(state.move_selection(Direction::Down));
        assert!(state.move_selection(Direction::Down));

        assert_eq!(state.selected(), Some(BookId(4)));
        assert_eq!(state.page(), 1);
        assert_eq!(state.page_count(), 2);
        assert_eq!(state.visible_range(), 4..7);
    }

    #[test]
    fn opens_reads_across_chapters_and_returns_to_the_shelf() {
        let mut app = App::new(2);
        assert_eq!(
            app.input(AppInput::Confirm),
            AppEffect::LoadChapter {
                book: BookId(0),
                spine_index: 0,
                target: PageTarget::First,
            }
        );
        let first = app.chapter_loaded(2, 2).unwrap();
        assert!(matches!(first, AppEffect::RenderReader(_)));
        assert!(matches!(
            app.input(AppInput::Confirm),
            AppEffect::RenderReader(_)
        ));
        assert_eq!(
            app.input(AppInput::Move(Direction::Right)),
            AppEffect::LoadChapter {
                book: BookId(0),
                spine_index: 1,
                target: PageTarget::First,
            }
        );
        app.chapter_loaded(2, 1).unwrap();
        assert_eq!(app.input(AppInput::Move(Direction::Right)), AppEffect::None);
        assert_eq!(app.input(AppInput::Back), AppEffect::RenderLibrary);
        assert_eq!(app.view(), AppView::Library);
    }

    #[test]
    fn reopening_a_book_returns_to_its_last_page() {
        let mut app = App::new(2);
        app.input(AppInput::Confirm);
        app.chapter_loaded(2, 3).unwrap();
        app.input(AppInput::Confirm);
        app.input(AppInput::Confirm);

        assert_eq!(app.input(AppInput::Back), AppEffect::RenderLibrary);
        assert_eq!(
            app.input(AppInput::Confirm),
            AppEffect::LoadChapter {
                book: BookId(0),
                spine_index: 0,
                target: PageTarget::Index(2),
            }
        );

        app.chapter_loaded(2, 3).unwrap();
        let AppView::Reader(location) = app.view() else {
            panic!("reader view expected");
        };
        assert_eq!(location.spine_index(), 0);
        assert_eq!(location.page_index(), 2);

        app.input(AppInput::Back);
        app.input(AppInput::Move(Direction::Right));
        assert_eq!(
            app.input(AppInput::Confirm),
            AppEffect::LoadChapter {
                book: BookId(1),
                spine_index: 0,
                target: PageTarget::First,
            }
        );
    }

    #[test]
    fn moving_backward_loads_the_last_page_of_the_previous_chapter() {
        let mut app = App::new(1);
        app.input(AppInput::Confirm);
        app.chapter_loaded(2, 1).unwrap();
        app.input(AppInput::Confirm);
        app.chapter_loaded(2, 3).unwrap();

        assert_eq!(
            app.input(AppInput::Move(Direction::Left)),
            AppEffect::LoadChapter {
                book: BookId(0),
                spine_index: 0,
                target: PageTarget::Last,
            }
        );
        app.chapter_loaded(2, 4).unwrap();
        let AppView::Reader(location) = app.view() else {
            panic!("reader view expected");
        };
        assert_eq!(location.page_index(), 3);
    }

    #[test]
    fn sleep_and_wake_restore_the_exact_reader_location() {
        let mut app = App::new(1);
        app.input(AppInput::Confirm);
        app.chapter_loaded(1, 4).unwrap();
        app.input(AppInput::Confirm);
        app.input(AppInput::Confirm);

        let resume = ResumePoint::Reader {
            book: BookId(0),
            spine_index: 0,
            page_index: 2,
        };
        assert_eq!(
            app.input(AppInput::Power),
            AppEffect::RenderSleep { resume }
        );
        assert_eq!(
            app.sleep_frame_ready().unwrap(),
            AppEffect::EnterDeepSleep { resume }
        );
        assert_eq!(
            app.wake(),
            AppEffect::LoadChapter {
                book: BookId(0),
                spine_index: 0,
                target: PageTarget::Index(2),
            }
        );
        app.chapter_loaded(1, 4).unwrap();
        let AppView::Reader(location) = app.view() else {
            panic!("reader view expected");
        };
        assert_eq!(location.page_index(), 2);
    }

    #[test]
    fn a_cold_boot_can_restore_a_retained_reader_location() {
        let resume = ResumePoint::Reader {
            book: BookId(1),
            spine_index: 3,
            page_index: 7,
        };
        let (app, effect) = App::from_resume(2, resume).unwrap();

        assert_eq!(app.view(), AppView::Loading);
        assert_eq!(
            effect,
            AppEffect::LoadChapter {
                book: BookId(1),
                spine_index: 3,
                target: PageTarget::Index(7),
            }
        );
    }
}
