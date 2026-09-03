use crate::power::BatteryStatus;

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
#[repr(u8)]
pub enum HomeItem {
    Books,
    Files,
    Settings,
}

impl HomeItem {
    pub const ALL: [Self; 3] = [Self::Books, Self::Files, Self::Settings];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Books),
            1 => Some(Self::Files),
            2 => Some(Self::Settings),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Books => "BOOKS",
            Self::Files => "FILES",
            Self::Settings => "SETTINGS",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HomeState {
    selected: HomeItem,
}

impl HomeState {
    pub const fn new() -> Self {
        Self {
            selected: HomeItem::Books,
        }
    }

    pub const fn with_selected(selected: HomeItem) -> Self {
        Self { selected }
    }

    pub const fn selected(self) -> HomeItem {
        self.selected
    }

    fn move_selection(&mut self, direction: Direction) -> bool {
        let next = match direction {
            Direction::Up => self.selected.index().saturating_sub(1),
            Direction::Down => (self.selected.index() + 1).min(HomeItem::ALL.len() - 1),
            Direction::Left | Direction::Right => self.selected.index(),
        };
        if next == self.selected.index() {
            return false;
        }
        self.selected = HomeItem::from_index(next).expect("home index is bounded");
        true
    }
}

impl Default for HomeState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilesState {
    book_count: usize,
    selected: Option<BookId>,
}

impl FilesState {
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
            Some(selected) => selected.index() / 8,
            None => 0,
        }
    }

    pub const fn page_count(self) -> usize {
        self.book_count.div_ceil(8)
    }

    pub fn visible_range(self) -> core::ops::Range<usize> {
        let start = self.page() * 8;
        start..(start + 8).min(self.book_count)
    }

    fn move_selection(&mut self, direction: Direction) -> bool {
        let Some(selected) = self.selected else {
            return false;
        };
        let next = match direction {
            Direction::Up | Direction::Left => selected.index().saturating_sub(1),
            Direction::Down | Direction::Right => (selected.index() + 1).min(self.book_count - 1),
        };
        if next == selected.index() {
            return false;
        }
        self.selected = Some(BookId(next));
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ReaderFont {
    NotoSerif,
    Compact,
    Mono,
}

impl ReaderFont {
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::NotoSerif),
            1 => Some(Self::Compact),
            2 => Some(Self::Mono),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::NotoSerif => "NOTO SERIF",
            Self::Compact => "COMPACT",
            Self::Mono => "MONO",
        }
    }

    const fn next(self, direction: Direction) -> Self {
        let index = cycle_index(self.index(), 3, direction);
        Self::from_index(index).expect("reader font index is bounded")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ReaderFontSize {
    Small,
    Medium,
    Large,
}

impl ReaderFontSize {
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Small),
            1 => Some(Self::Medium),
            2 => Some(Self::Large),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Small => "SMALL",
            Self::Medium => "MEDIUM",
            Self::Large => "LARGE",
        }
    }

    const fn next(self, direction: Direction) -> Self {
        let index = cycle_index(self.index(), 3, direction);
        Self::from_index(index).expect("reader size index is bounded")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ReaderSpacing {
    Compact,
    Normal,
    Relaxed,
}

impl ReaderSpacing {
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Compact),
            1 => Some(Self::Normal),
            2 => Some(Self::Relaxed),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Compact => "COMPACT",
            Self::Normal => "NORMAL",
            Self::Relaxed => "RELAXED",
        }
    }

    const fn next(self, direction: Direction) -> Self {
        let index = cycle_index(self.index(), 3, direction);
        Self::from_index(index).expect("reader spacing index is bounded")
    }
}

const fn cycle_index(current: usize, length: usize, direction: Direction) -> usize {
    match direction {
        Direction::Left => {
            if current == 0 {
                length - 1
            } else {
                current - 1
            }
        }
        Direction::Right => (current + 1) % length,
        Direction::Up | Direction::Down => current,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReaderPreferences {
    font: ReaderFont,
    size: ReaderFontSize,
    spacing: ReaderSpacing,
}

impl ReaderPreferences {
    pub const fn new(font: ReaderFont, size: ReaderFontSize, spacing: ReaderSpacing) -> Self {
        Self {
            font,
            size,
            spacing,
        }
    }

    pub const fn font(self) -> ReaderFont {
        self.font
    }

    pub const fn size(self) -> ReaderFontSize {
        self.size
    }

    pub const fn spacing(self) -> ReaderSpacing {
        self.spacing
    }

    pub const fn packed(self) -> u32 {
        self.font.index() as u32
            | (self.size.index() as u32) << 8
            | (self.spacing.index() as u32) << 16
    }

    pub const fn from_packed(value: u32) -> Option<Self> {
        if value & 0xFF00_0000 != 0 {
            return None;
        }
        let Some(font) = ReaderFont::from_index((value & 0xFF) as usize) else {
            return None;
        };
        let Some(size) = ReaderFontSize::from_index(((value >> 8) & 0xFF) as usize) else {
            return None;
        };
        let Some(spacing) = ReaderSpacing::from_index(((value >> 16) & 0xFF) as usize) else {
            return None;
        };
        Some(Self::new(font, size, spacing))
    }
}

impl Default for ReaderPreferences {
    fn default() -> Self {
        Self::new(
            ReaderFont::NotoSerif,
            ReaderFontSize::Medium,
            ReaderSpacing::Normal,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SettingsItem {
    Font,
    Size,
    Spacing,
    Apply,
}

impl SettingsItem {
    pub const ALL: [Self; 4] = [Self::Font, Self::Size, Self::Spacing, Self::Apply];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Font),
            1 => Some(Self::Size),
            2 => Some(Self::Spacing),
            3 => Some(Self::Apply),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Font => "FONT",
            Self::Size => "TEXT SIZE",
            Self::Spacing => "LINE SPACING",
            Self::Apply => "APPLY SETTINGS",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsState {
    selected: SettingsItem,
    draft: ReaderPreferences,
}

impl SettingsState {
    pub const fn new(preferences: ReaderPreferences) -> Self {
        Self {
            selected: SettingsItem::Font,
            draft: preferences,
        }
    }

    pub const fn with_state(selected: SettingsItem, draft: ReaderPreferences) -> Self {
        Self { selected, draft }
    }

    pub const fn selected(self) -> SettingsItem {
        self.selected
    }

    pub const fn draft(self) -> ReaderPreferences {
        self.draft
    }

    fn input(&mut self, direction: Direction) -> bool {
        match direction {
            Direction::Up | Direction::Down => {
                let next = match direction {
                    Direction::Up => self.selected.index().saturating_sub(1),
                    Direction::Down => (self.selected.index() + 1).min(SettingsItem::ALL.len() - 1),
                    Direction::Left | Direction::Right => unreachable!(),
                };
                if next == self.selected.index() {
                    return false;
                }
                self.selected = SettingsItem::from_index(next).expect("settings index is bounded");
                true
            }
            Direction::Left | Direction::Right => {
                let next = match self.selected {
                    SettingsItem::Font => {
                        Self::with_font(self.draft, self.draft.font.next(direction))
                    }
                    SettingsItem::Size => {
                        Self::with_size(self.draft, self.draft.size.next(direction))
                    }
                    SettingsItem::Spacing => {
                        Self::with_spacing(self.draft, self.draft.spacing.next(direction))
                    }
                    SettingsItem::Apply => return false,
                };
                if next == self.draft {
                    return false;
                }
                self.draft = next;
                true
            }
        }
    }

    const fn with_font(preferences: ReaderPreferences, font: ReaderFont) -> ReaderPreferences {
        ReaderPreferences::new(font, preferences.size, preferences.spacing)
    }

    const fn with_size(preferences: ReaderPreferences, size: ReaderFontSize) -> ReaderPreferences {
        ReaderPreferences::new(preferences.font, size, preferences.spacing)
    }

    const fn with_spacing(
        preferences: ReaderPreferences,
        spacing: ReaderSpacing,
    ) -> ReaderPreferences {
        ReaderPreferences::new(preferences.font, preferences.size, spacing)
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
    Progress {
        page_index: usize,
        page_count: usize,
    },
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
#[repr(u8)]
pub enum BookOrigin {
    Books,
    Files,
}

impl BookOrigin {
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Books),
            1 => Some(Self::Files),
            _ => None,
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadingSession {
    location: ReadingLocation,
    origin: BookOrigin,
}

impl ReadingSession {
    pub const fn location(self) -> ReadingLocation {
        self.location
    }

    pub const fn origin(self) -> BookOrigin {
        self.origin
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumePoint {
    Home {
        selected: HomeItem,
    },
    Books {
        selected: Option<BookId>,
    },
    Files {
        selected: Option<BookId>,
    },
    Settings {
        selected: SettingsItem,
        draft: ReaderPreferences,
    },
    Reader {
        book: BookId,
        spine_index: usize,
        page_index: usize,
        origin: BookOrigin,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppView {
    Home(HomeState),
    Library,
    Files(FilesState),
    Settings(SettingsState),
    Loading,
    Reader(ReadingSession),
    Error { book: BookId, origin: BookOrigin },
    Sleeping { resume: ResumePoint },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppEffect {
    None,
    RenderHome,
    RenderLibrary,
    RenderFiles,
    RenderSettings,
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
    origin: BookOrigin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReadingCheckpoint {
    book: BookId,
    spine_index: usize,
    page_index: usize,
    page_count: usize,
    preferences: ReaderPreferences,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct App {
    library: LibraryState,
    files: FilesState,
    home: HomeState,
    view: AppView,
    preferences: ReaderPreferences,
    battery: BatteryStatus,
    pending: Option<PendingChapter>,
    reading_checkpoint: Option<ReadingCheckpoint>,
}

impl App {
    pub const fn new(book_count: usize) -> Self {
        Self::with_preferences(
            book_count,
            ReaderPreferences::new(
                ReaderFont::NotoSerif,
                ReaderFontSize::Medium,
                ReaderSpacing::Normal,
            ),
        )
    }

    pub const fn with_preferences(book_count: usize, preferences: ReaderPreferences) -> Self {
        let home = HomeState::new();
        Self {
            library: LibraryState::new(book_count),
            files: FilesState::new(book_count),
            home,
            view: AppView::Home(home),
            preferences,
            battery: BatteryStatus::unknown(),
            pending: None,
            reading_checkpoint: None,
        }
    }

    pub fn from_resume(
        book_count: usize,
        preferences: ReaderPreferences,
        resume: ResumePoint,
    ) -> Result<(Self, AppEffect), AppStateError> {
        let mut app = Self::with_preferences(book_count, preferences);
        let effect = match resume {
            ResumePoint::Home { selected } => {
                app.home = HomeState::with_selected(selected);
                app.view = AppView::Home(app.home);
                AppEffect::RenderHome
            }
            ResumePoint::Books { selected: None } if book_count == 0 => {
                app.view = AppView::Library;
                AppEffect::RenderLibrary
            }
            ResumePoint::Books {
                selected: Some(selected),
            } => {
                app.library = LibraryState::with_selected(book_count, selected.index())
                    .map_err(|_| AppStateError::BookOutOfBounds)?;
                app.view = AppView::Library;
                AppEffect::RenderLibrary
            }
            ResumePoint::Books { selected: None } => {
                app.view = AppView::Library;
                AppEffect::RenderLibrary
            }
            ResumePoint::Files { selected: None } if book_count == 0 => {
                app.view = AppView::Files(app.files);
                AppEffect::RenderFiles
            }
            ResumePoint::Files {
                selected: Some(selected),
            } => {
                app.files = FilesState::with_selected(book_count, selected.index())
                    .map_err(|_| AppStateError::BookOutOfBounds)?;
                app.view = AppView::Files(app.files);
                AppEffect::RenderFiles
            }
            ResumePoint::Files { selected: None } => {
                app.view = AppView::Files(app.files);
                AppEffect::RenderFiles
            }
            ResumePoint::Settings { selected, draft } => {
                let settings = SettingsState::with_state(selected, draft);
                app.home = HomeState::with_selected(HomeItem::Settings);
                app.view = AppView::Settings(settings);
                AppEffect::RenderSettings
            }
            ResumePoint::Reader {
                book,
                spine_index,
                page_index,
                origin,
            } => {
                app.validate_book(book)?;
                app.select_book(book);
                app.reading_checkpoint = Some(ReadingCheckpoint {
                    book,
                    spine_index,
                    page_index,
                    page_count: page_index + 1,
                    preferences,
                });
                app.request_chapter(book, spine_index, PageTarget::Index(page_index), origin)
            }
        };
        Ok((app, effect))
    }

    pub const fn library(self) -> LibraryState {
        self.library
    }

    pub const fn files(self) -> FilesState {
        self.files
    }

    pub const fn home(self) -> HomeState {
        self.home
    }

    pub const fn view(self) -> AppView {
        self.view
    }

    pub const fn preferences(self) -> ReaderPreferences {
        self.preferences
    }

    pub const fn battery(self) -> BatteryStatus {
        self.battery
    }

    pub fn set_battery(&mut self, battery: BatteryStatus) -> bool {
        if self.battery == battery {
            return false;
        }
        self.battery = battery;
        true
    }

    pub fn resume_point(self) -> ResumePoint {
        match self.view {
            AppView::Home(home) => ResumePoint::Home {
                selected: home.selected,
            },
            AppView::Library => ResumePoint::Books {
                selected: self.library.selected,
            },
            AppView::Files(files) => ResumePoint::Files {
                selected: files.selected,
            },
            AppView::Settings(settings) => ResumePoint::Settings {
                selected: settings.selected,
                draft: settings.draft,
            },
            AppView::Reader(session) => ResumePoint::Reader {
                book: session.location.book,
                spine_index: session.location.spine_index,
                page_index: session.location.page_index,
                origin: session.origin,
            },
            AppView::Sleeping { resume } => resume,
            AppView::Loading => self.pending.map_or(
                ResumePoint::Home {
                    selected: self.home.selected,
                },
                |pending| self.origin_resume(pending.origin),
            ),
            AppView::Error { origin, .. } => self.origin_resume(origin),
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
            (AppView::Home(mut home), AppInput::Move(direction)) => {
                if !home.move_selection(direction) {
                    return AppEffect::None;
                }
                self.home = home;
                self.view = AppView::Home(home);
                AppEffect::RenderHome
            }
            (AppView::Home(home), AppInput::Confirm) => match home.selected {
                HomeItem::Books => {
                    self.view = AppView::Library;
                    AppEffect::RenderLibrary
                }
                HomeItem::Files => {
                    self.view = AppView::Files(self.files);
                    AppEffect::RenderFiles
                }
                HomeItem::Settings => {
                    self.view = AppView::Settings(SettingsState::new(self.preferences));
                    AppEffect::RenderSettings
                }
            },
            (AppView::Library, AppInput::Move(direction)) => {
                if self.library.move_selection(direction) {
                    AppEffect::RenderLibrary
                } else {
                    AppEffect::None
                }
            }
            (AppView::Library, AppInput::Confirm) => self.open_selected(BookOrigin::Books),
            (AppView::Library, AppInput::Back) => self.return_home(HomeItem::Books),
            (AppView::Files(mut files), AppInput::Move(direction)) => {
                if !files.move_selection(direction) {
                    return AppEffect::None;
                }
                self.files = files;
                self.view = AppView::Files(files);
                AppEffect::RenderFiles
            }
            (AppView::Files(_), AppInput::Confirm) => self.open_selected(BookOrigin::Files),
            (AppView::Files(_), AppInput::Back) => self.return_home(HomeItem::Files),
            (AppView::Settings(mut settings), AppInput::Move(direction)) => {
                if !settings.input(direction) {
                    return AppEffect::None;
                }
                self.view = AppView::Settings(settings);
                AppEffect::RenderSettings
            }
            (AppView::Settings(settings), AppInput::Confirm)
                if settings.selected == SettingsItem::Apply =>
            {
                self.preferences = settings.draft;
                self.return_home(HomeItem::Settings)
            }
            (AppView::Settings(mut settings), AppInput::Confirm) => {
                settings.input(Direction::Right);
                self.view = AppView::Settings(settings);
                AppEffect::RenderSettings
            }
            (AppView::Settings(_), AppInput::Back) => self.return_home(HomeItem::Settings),
            (AppView::Reader(session), AppInput::Move(Direction::Right | Direction::Down))
            | (AppView::Reader(session), AppInput::Confirm) => self.next_page(session),
            (AppView::Reader(session), AppInput::Move(Direction::Left | Direction::Up)) => {
                self.previous_page(session)
            }
            (AppView::Reader(session), AppInput::Back) => self.return_to_origin(session.origin),
            (AppView::Loading, AppInput::Back) => {
                let origin = self
                    .pending
                    .map_or(BookOrigin::Books, |pending| pending.origin);
                self.pending = None;
                self.return_to_origin(origin)
            }
            (AppView::Error { origin, .. }, AppInput::Back) => self.return_to_origin(origin),
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
            PageTarget::Progress {
                page_index: previous_index,
                page_count: previous_count,
            } => remap_page(previous_index, previous_count, page_count),
        };
        let location = ReadingLocation {
            book: pending.book,
            spine_index: pending.spine_index,
            spine_count,
            page_index,
            page_count,
        };
        let session = ReadingSession {
            location,
            origin: pending.origin,
        };
        self.view = AppView::Reader(session);
        self.reading_checkpoint = Some(ReadingCheckpoint {
            book: location.book,
            spine_index: location.spine_index,
            page_index: location.page_index,
            page_count: location.page_count,
            preferences: self.preferences,
        });
        Ok(AppEffect::RenderReader(location))
    }

    pub fn chapter_failed(&mut self) -> Result<AppEffect, AppStateError> {
        let pending = self
            .pending
            .take()
            .ok_or(AppStateError::UnexpectedChapter)?;
        self.view = AppView::Error {
            book: pending.book,
            origin: pending.origin,
        };
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
            ResumePoint::Home { selected } => {
                self.home = HomeState::with_selected(selected);
                self.view = AppView::Home(self.home);
                AppEffect::RenderHome
            }
            ResumePoint::Books { selected } => {
                if let Some(selected) = selected {
                    self.select_book(selected);
                }
                self.view = AppView::Library;
                AppEffect::RenderLibrary
            }
            ResumePoint::Files { selected } => {
                if let Some(selected) = selected {
                    self.select_book(selected);
                }
                self.view = AppView::Files(self.files);
                AppEffect::RenderFiles
            }
            ResumePoint::Settings { selected, draft } => {
                let settings = SettingsState::with_state(selected, draft);
                self.view = AppView::Settings(settings);
                AppEffect::RenderSettings
            }
            ResumePoint::Reader {
                book,
                spine_index,
                page_index,
                origin,
            } => self.request_chapter(book, spine_index, PageTarget::Index(page_index), origin),
        }
    }

    fn open_selected(&mut self, origin: BookOrigin) -> AppEffect {
        let selected = match origin {
            BookOrigin::Books => self.library.selected,
            BookOrigin::Files => self.files.selected,
        };
        let Some(book) = selected else {
            return AppEffect::None;
        };
        self.select_book(book);
        match self
            .reading_checkpoint
            .filter(|checkpoint| checkpoint.book == book)
        {
            Some(checkpoint) if checkpoint.preferences == self.preferences => self.request_chapter(
                book,
                checkpoint.spine_index,
                PageTarget::Index(checkpoint.page_index),
                origin,
            ),
            Some(checkpoint) => self.request_chapter(
                book,
                checkpoint.spine_index,
                PageTarget::Progress {
                    page_index: checkpoint.page_index,
                    page_count: checkpoint.page_count,
                },
                origin,
            ),
            None => self.request_chapter(book, 0, PageTarget::First, origin),
        }
    }

    fn next_page(&mut self, session: ReadingSession) -> AppEffect {
        let location = session.location;
        if location.page_index + 1 < location.page_count {
            let next = ReadingLocation {
                page_index: location.page_index + 1,
                ..location
            };
            self.set_reading_session(next, session.origin)
        } else if location.spine_index + 1 < location.spine_count {
            self.request_chapter(
                location.book,
                location.spine_index + 1,
                PageTarget::First,
                session.origin,
            )
        } else {
            AppEffect::None
        }
    }

    fn previous_page(&mut self, session: ReadingSession) -> AppEffect {
        let location = session.location;
        if location.page_index > 0 {
            let previous = ReadingLocation {
                page_index: location.page_index - 1,
                ..location
            };
            self.set_reading_session(previous, session.origin)
        } else if location.spine_index > 0 {
            self.request_chapter(
                location.book,
                location.spine_index - 1,
                PageTarget::Last,
                session.origin,
            )
        } else {
            AppEffect::None
        }
    }

    fn set_reading_session(&mut self, location: ReadingLocation, origin: BookOrigin) -> AppEffect {
        self.view = AppView::Reader(ReadingSession { location, origin });
        self.reading_checkpoint = Some(ReadingCheckpoint {
            book: location.book,
            spine_index: location.spine_index,
            page_index: location.page_index,
            page_count: location.page_count,
            preferences: self.preferences,
        });
        AppEffect::RenderReader(location)
    }

    fn request_chapter(
        &mut self,
        book: BookId,
        spine_index: usize,
        target: PageTarget,
        origin: BookOrigin,
    ) -> AppEffect {
        self.view = AppView::Loading;
        self.pending = Some(PendingChapter {
            book,
            spine_index,
            target,
            origin,
        });
        AppEffect::LoadChapter {
            book,
            spine_index,
            target,
        }
    }

    fn return_home(&mut self, selected: HomeItem) -> AppEffect {
        self.home = HomeState::with_selected(selected);
        self.view = AppView::Home(self.home);
        self.pending = None;
        AppEffect::RenderHome
    }

    fn return_to_origin(&mut self, origin: BookOrigin) -> AppEffect {
        self.pending = None;
        match origin {
            BookOrigin::Books => {
                self.view = AppView::Library;
                AppEffect::RenderLibrary
            }
            BookOrigin::Files => {
                self.view = AppView::Files(self.files);
                AppEffect::RenderFiles
            }
        }
    }

    fn origin_resume(self, origin: BookOrigin) -> ResumePoint {
        match origin {
            BookOrigin::Books => ResumePoint::Books {
                selected: self.library.selected,
            },
            BookOrigin::Files => ResumePoint::Files {
                selected: self.files.selected,
            },
        }
    }

    fn select_book(&mut self, book: BookId) {
        if let Ok(library) = LibraryState::with_selected(self.library.book_count, book.index()) {
            self.library = library;
        }
        if let Ok(files) = FilesState::with_selected(self.files.book_count, book.index()) {
            self.files = files;
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

fn remap_page(page_index: usize, old_count: usize, new_count: usize) -> usize {
    if old_count <= 1 || new_count <= 1 {
        return 0;
    }
    page_index.min(old_count - 1) * (new_count - 1) / (old_count - 1)
}

#[cfg(test)]
mod tests {
    use super::{
        App, AppEffect, AppInput, AppView, BookOrigin, Direction, FilesState, HomeItem,
        LibraryState, PageTarget, ReaderFont, ReaderFontSize, ReaderPreferences, ReaderSpacing,
        ResumePoint, SettingsItem,
    };
    use crate::{input::UsbState, power::BatteryStatus};

    fn open_books(app: &mut App) {
        assert_eq!(app.input(AppInput::Confirm), AppEffect::RenderLibrary);
    }

    fn open_first_book(app: &mut App, pages: usize) {
        open_books(app);
        assert!(matches!(
            app.input(AppInput::Confirm),
            AppEffect::LoadChapter { .. }
        ));
        assert!(matches!(
            app.chapter_loaded(3, pages).unwrap(),
            AppEffect::RenderReader(_)
        ));
    }

    #[test]
    fn reader_preferences_round_trip_the_packed_boundary() {
        let preferences = ReaderPreferences {
            font: ReaderFont::Mono,
            size: ReaderFontSize::Large,
            spacing: ReaderSpacing::Relaxed,
        };
        assert_eq!(
            ReaderPreferences::from_packed(preferences.packed()),
            Some(preferences)
        );
        assert_eq!(ReaderPreferences::from_packed(0xFF00_0000), None);
    }

    #[test]
    fn an_empty_library_has_no_selection_or_pages() {
        let state = LibraryState::new(0);
        assert_eq!(state.selected(), None);
        assert_eq!(state.visible_range(), 0..0);
        assert_eq!(state.page_count(), 0);
    }

    #[test]
    fn directional_navigation_stays_within_the_catalog() {
        let mut state = LibraryState::new(3);
        assert!(!state.move_selection(Direction::Left));
        assert!(state.move_selection(Direction::Right));
        assert_eq!(state.selected().unwrap().index(), 1);
        assert!(state.move_selection(Direction::Down));
        assert_eq!(state.selected().unwrap().index(), 2);
        assert!(!state.move_selection(Direction::Down));
    }

    #[test]
    fn moving_beyond_four_books_advances_the_visible_page() {
        let mut state = LibraryState::new(7);
        state.move_selection(Direction::Down);
        state.move_selection(Direction::Down);
        assert_eq!(state.page(), 1);
        assert_eq!(state.visible_range(), 4..7);
    }

    #[test]
    fn starts_at_home_and_opens_each_primary_section() {
        let mut app = App::new(4);
        assert_eq!(app.view(), AppView::Home(app.home()));
        assert_eq!(app.input(AppInput::Confirm), AppEffect::RenderLibrary);
        assert_eq!(app.input(AppInput::Back), AppEffect::RenderHome);

        app.input(AppInput::Move(Direction::Down));
        assert_eq!(app.home().selected(), HomeItem::Files);
        assert_eq!(app.input(AppInput::Confirm), AppEffect::RenderFiles);
        assert_eq!(app.view(), AppView::Files(FilesState::new(4)));
        assert_eq!(app.input(AppInput::Back), AppEffect::RenderHome);

        app.input(AppInput::Move(Direction::Down));
        assert_eq!(app.home().selected(), HomeItem::Settings);
        assert_eq!(app.input(AppInput::Confirm), AppEffect::RenderSettings);
        assert!(matches!(app.view(), AppView::Settings(_)));
    }

    #[test]
    fn settings_use_a_draft_and_only_apply_from_the_apply_row() {
        let mut app = App::new(1);
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Confirm);
        app.input(AppInput::Move(Direction::Right));
        assert_eq!(app.preferences(), ReaderPreferences::default());
        app.input(AppInput::Back);
        assert_eq!(app.preferences(), ReaderPreferences::default());

        app.input(AppInput::Confirm);
        app.input(AppInput::Move(Direction::Right));
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Move(Direction::Right));
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Move(Direction::Right));
        app.input(AppInput::Move(Direction::Down));
        assert!(matches!(
            app.view(),
            AppView::Settings(settings) if settings.selected() == SettingsItem::Apply
        ));
        assert_eq!(app.input(AppInput::Confirm), AppEffect::RenderHome);
        assert_eq!(
            app.preferences(),
            ReaderPreferences::new(
                ReaderFont::Compact,
                ReaderFontSize::Large,
                ReaderSpacing::Relaxed,
            )
        );
    }

    #[test]
    fn files_open_the_same_reader_and_back_returns_to_files() {
        let mut app = App::new(2);
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Confirm);
        app.input(AppInput::Move(Direction::Down));
        assert!(matches!(
            app.input(AppInput::Confirm),
            AppEffect::LoadChapter { book, .. } if book.index() == 1
        ));
        app.chapter_loaded(1, 2).unwrap();
        assert_eq!(app.input(AppInput::Back), AppEffect::RenderFiles);
        assert!(
            matches!(app.view(), AppView::Files(files) if files.selected().unwrap().index() == 1)
        );
    }

    #[test]
    fn reader_turns_pages_and_crosses_chapter_boundaries() {
        let mut app = App::new(1);
        open_first_book(&mut app, 2);
        assert!(matches!(
            app.input(AppInput::Move(Direction::Right)),
            AppEffect::RenderReader(location) if location.page_index() == 1
        ));
        assert!(matches!(
            app.input(AppInput::Move(Direction::Right)),
            AppEffect::LoadChapter {
                spine_index: 1,
                target: PageTarget::First,
                ..
            }
        ));
        let AppEffect::RenderReader(location) = app.chapter_loaded(3, 4).unwrap() else {
            panic!("expected reader render");
        };
        assert_eq!(location.spine_index(), 1);
        assert_eq!(location.page_index(), 0);
    }

    #[test]
    fn changed_typography_maps_the_old_progress_into_the_new_page_count() {
        let mut app = App::new(1);
        open_first_book(&mut app, 5);
        app.input(AppInput::Move(Direction::Right));
        app.input(AppInput::Move(Direction::Right));
        app.input(AppInput::Back);
        app.input(AppInput::Back);
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Confirm);
        app.input(AppInput::Move(Direction::Right));
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Confirm);
        app.input(AppInput::Move(Direction::Up));
        app.input(AppInput::Move(Direction::Up));
        app.input(AppInput::Confirm);
        let AppEffect::LoadChapter {
            target:
                PageTarget::Progress {
                    page_index,
                    page_count,
                },
            ..
        } = app.input(AppInput::Confirm)
        else {
            panic!("expected progress load");
        };
        assert_eq!((page_index, page_count), (2, 5));
        let AppEffect::RenderReader(location) = app.chapter_loaded(3, 9).unwrap() else {
            panic!("expected reader render");
        };
        assert_eq!(location.page_index(), 4);
    }

    #[test]
    fn sleep_and_wake_restore_settings_and_reader_origins() {
        let preferences = ReaderPreferences::new(
            ReaderFont::Mono,
            ReaderFontSize::Large,
            ReaderSpacing::Compact,
        );
        let resume = ResumePoint::Reader {
            book: super::BookId::new(1),
            spine_index: 2,
            page_index: 3,
            origin: BookOrigin::Files,
        };
        let (mut app, effect) = App::from_resume(2, preferences, resume).unwrap();
        assert!(matches!(effect, AppEffect::LoadChapter { .. }));
        app.chapter_loaded(4, 7).unwrap();
        assert!(matches!(
            app.input(AppInput::Power),
            AppEffect::RenderSleep {
                resume: ResumePoint::Reader {
                    origin: BookOrigin::Files,
                    ..
                }
            }
        ));
        assert!(matches!(
            app.sleep_frame_ready().unwrap(),
            AppEffect::EnterDeepSleep { .. }
        ));
        assert!(matches!(app.wake(), AppEffect::LoadChapter { .. }));
        app.chapter_loaded(4, 7).unwrap();
        assert_eq!(app.input(AppInput::Back), AppEffect::RenderFiles);
        assert_eq!(app.preferences(), preferences);
    }

    #[test]
    fn battery_status_is_shared_application_state() {
        let mut app = App::new(0);
        let status = BatteryStatus::from_percent(42, UsbState::Disconnected);
        assert!(app.set_battery(status));
        assert!(!app.set_battery(status));
        assert_eq!(app.battery(), status);
    }
}
