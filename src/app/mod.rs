use crate::power::{BatteryLevel, BatteryStatus};

#[cfg(test)]
mod reader_drawer_tests;

const BOOKS_PER_SHELF_PAGE: usize = 4;
const SHELF_COLUMNS: usize = 2;
const BATTERY_REFRESH_PERCENT_DELTA: u8 = 5;

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
pub struct ImageId(usize);

impl ImageId {
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileId(usize);

impl FileId {
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
            Self::Books => "Books",
            Self::Files => "Files",
            Self::Settings => "Settings",
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
    file_count: usize,
    selected: Option<FileId>,
}

impl FilesState {
    pub const fn new(file_count: usize) -> Self {
        Self {
            file_count,
            selected: if file_count == 0 {
                None
            } else {
                Some(FileId(0))
            },
        }
    }

    pub const fn with_selected(
        file_count: usize,
        selected: usize,
    ) -> Result<Self, SelectionOutOfBounds> {
        if selected >= file_count {
            return Err(SelectionOutOfBounds);
        }
        Ok(Self {
            file_count,
            selected: Some(FileId(selected)),
        })
    }

    pub const fn file_count(self) -> usize {
        self.file_count
    }

    pub const fn selected(self) -> Option<FileId> {
        self.selected
    }

    pub const fn page(self) -> usize {
        match self.selected {
            Some(selected) => selected.index() / 8,
            None => 0,
        }
    }

    pub const fn page_count(self) -> usize {
        self.file_count.div_ceil(8)
    }

    pub fn visible_range(self) -> core::ops::Range<usize> {
        let start = self.page() * 8;
        start..(start + 8).min(self.file_count)
    }

    fn move_selection(&mut self, direction: Direction) -> bool {
        let Some(selected) = self.selected else {
            return false;
        };
        let next = match direction {
            Direction::Up | Direction::Left => selected.index().saturating_sub(1),
            Direction::Down | Direction::Right => (selected.index() + 1).min(self.file_count - 1),
        };
        if next == selected.index() {
            return false;
        }
        self.selected = Some(FileId(next));
        true
    }

    fn replace_count(&mut self, file_count: usize) {
        self.file_count = file_count;
        self.selected = match (self.selected, file_count) {
            (_, 0) => None,
            (Some(selected), count) => Some(FileId(selected.index().min(count - 1))),
            (None, _) => Some(FileId(0)),
        };
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
            Self::NotoSerif => "Noto Serif",
            Self::Compact => "Compact",
            Self::Mono => "Mono",
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
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
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
            Self::Compact => "Compact",
            Self::Normal => "Normal",
            Self::Relaxed => "Relaxed",
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
pub enum SleepScreenMode {
    Automatic,
    Custom,
    BookCover,
}

impl SleepScreenMode {
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Automatic),
            1 => Some(Self::Custom),
            2 => Some(Self::BookCover),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Custom => "Custom image",
            Self::BookCover => "Book cover",
        }
    }

    const fn next(self, direction: Direction) -> Self {
        let index = cycle_index(self.index(), 3, direction);
        Self::from_index(index).expect("sleep screen mode index is bounded")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppPreferences {
    reader: ReaderPreferences,
    sleep_screen: SleepScreenMode,
}

impl AppPreferences {
    pub const fn new(reader: ReaderPreferences, sleep_screen: SleepScreenMode) -> Self {
        Self {
            reader,
            sleep_screen,
        }
    }

    pub const fn reader(self) -> ReaderPreferences {
        self.reader
    }

    pub const fn sleep_screen(self) -> SleepScreenMode {
        self.sleep_screen
    }

    pub const fn packed(self) -> u32 {
        self.reader.packed() | (self.sleep_screen.index() as u32) << 24
    }

    pub const fn from_packed(value: u32) -> Option<Self> {
        let Some(reader) = ReaderPreferences::from_packed(value & 0x00FF_FFFF) else {
            return None;
        };
        let Some(sleep_screen) = SleepScreenMode::from_index((value >> 24) as usize) else {
            return None;
        };
        Some(Self::new(reader, sleep_screen))
    }

    const fn with_reader(self, reader: ReaderPreferences) -> Self {
        Self::new(reader, self.sleep_screen)
    }

    const fn with_sleep_screen(self, sleep_screen: SleepScreenMode) -> Self {
        Self::new(self.reader, sleep_screen)
    }
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self::new(ReaderPreferences::default(), SleepScreenMode::Automatic)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum SettingsItem {
    Font,
    Size,
    Spacing,
    SleepScreen,
    Apply,
}

impl SettingsItem {
    pub const ALL: [Self; 5] = [
        Self::Font,
        Self::Size,
        Self::Spacing,
        Self::SleepScreen,
        Self::Apply,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn from_index(index: usize) -> Option<Self> {
        if index < Self::ALL.len() {
            Some(Self::ALL[index])
        } else {
            None
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Font => "Font",
            Self::Size => "Text size",
            Self::Spacing => "Line spacing",
            Self::SleepScreen => "Sleep screen",
            Self::Apply => "Save settings",
        }
    }

    pub const fn value(self, preferences: AppPreferences) -> Option<&'static str> {
        match self {
            Self::Font => Some(preferences.reader().font().label()),
            Self::Size => Some(preferences.reader().size().label()),
            Self::Spacing => Some(preferences.reader().spacing().label()),
            Self::SleepScreen => Some(preferences.sleep_screen().label()),
            Self::Apply => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SettingsState {
    selected: SettingsItem,
    draft: AppPreferences,
}

impl SettingsState {
    pub const fn new(preferences: AppPreferences) -> Self {
        Self {
            selected: SettingsItem::Font,
            draft: preferences,
        }
    }

    pub const fn with_state(selected: SettingsItem, draft: AppPreferences) -> Self {
        Self { selected, draft }
    }

    pub const fn selected(self) -> SettingsItem {
        self.selected
    }

    pub const fn draft(self) -> AppPreferences {
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
                let reader = self.draft.reader;
                let next = match self.selected {
                    SettingsItem::Font => self.draft.with_reader(ReaderPreferences::new(
                        reader.font.next(direction),
                        reader.size,
                        reader.spacing,
                    )),
                    SettingsItem::Size => self.draft.with_reader(ReaderPreferences::new(
                        reader.font,
                        reader.size.next(direction),
                        reader.spacing,
                    )),
                    SettingsItem::Spacing => self.draft.with_reader(ReaderPreferences::new(
                        reader.font,
                        reader.size,
                        reader.spacing.next(direction),
                    )),
                    SettingsItem::SleepScreen => self
                        .draft
                        .with_sleep_screen(self.draft.sleep_screen.next(direction)),
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
#[repr(u8)]
pub enum ReaderControl {
    Page,
    Chapter,
    Font,
    Size,
    Spacing,
}

impl ReaderControl {
    pub const ALL: [Self; 5] = [
        Self::Page,
        Self::Chapter,
        Self::Font,
        Self::Size,
        Self::Spacing,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Page => "Page in chapter",
            Self::Chapter => "Chapter",
            Self::Font => "Font",
            Self::Size => "Text size",
            Self::Spacing => "Line spacing",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReaderDrawer {
    session: ReadingSession,
    selected: ReaderControl,
    page: usize,
    chapter: usize,
    preferences: ReaderPreferences,
}

impl ReaderDrawer {
    pub const fn new(session: ReadingSession, preferences: ReaderPreferences) -> Self {
        Self {
            session,
            selected: ReaderControl::Page,
            page: session.location.page_index,
            chapter: session.location.spine_index,
            preferences,
        }
    }

    pub const fn session(self) -> ReadingSession {
        self.session
    }
    pub const fn selected(self) -> ReaderControl {
        self.selected
    }
    pub const fn page(self) -> usize {
        self.page
    }
    pub const fn chapter(self) -> usize {
        self.chapter
    }
    pub const fn preferences(self) -> ReaderPreferences {
        self.preferences
    }

    fn input(&mut self, direction: Direction) {
        if matches!(direction, Direction::Up | Direction::Down) {
            let index = self.selected as usize;
            let next = if direction == Direction::Up {
                index.saturating_sub(1)
            } else {
                (index + 1).min(ReaderControl::ALL.len() - 1)
            };
            self.selected = ReaderControl::ALL[next];
            return;
        }
        match self.selected {
            ReaderControl::Page => {
                let count = self.session.location.page_count;
                let step = count.div_ceil(20);
                self.page = if direction == Direction::Left {
                    self.page.saturating_sub(step)
                } else {
                    self.page.saturating_add(step).min(count - 1)
                };
            }
            ReaderControl::Chapter => {
                self.chapter = if direction == Direction::Left {
                    self.chapter.saturating_sub(1)
                } else {
                    (self.chapter + 1).min(self.session.location.spine_count - 1)
                };
            }
            ReaderControl::Font => self.preferences.font = self.preferences.font.next(direction),
            ReaderControl::Size => self.preferences.size = self.preferences.size.next(direction),
            ReaderControl::Spacing => {
                self.preferences.spacing = self.preferences.spacing.next(direction)
            }
        }
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
        selected: Option<FileId>,
    },
    Settings {
        selected: SettingsItem,
        draft: AppPreferences,
    },
    Reader {
        book: BookId,
        spine_index: usize,
        page_index: usize,
        origin: BookOrigin,
    },
    Image {
        image: ImageId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppView {
    Home(HomeState),
    Library,
    Files(FilesState),
    Settings(SettingsState),
    Loading(PendingChapter),
    Reader(ReadingSession),
    ReaderDrawer(ReaderDrawer),
    Image(ImageId),
    Error { book: BookId, origin: BookOrigin },
    Sleeping { resume: ResumePoint },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SleepScreenSource {
    CustomImage(ImageId),
    BookCover(BookId),
    BuiltIn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SleepScreenPlan {
    sources: [SleepScreenSource; 2],
    length: usize,
}

impl SleepScreenPlan {
    pub const fn resolve(
        mode: SleepScreenMode,
        resume: ResumePoint,
        selected_image: Option<ImageId>,
        book_count: usize,
    ) -> Self {
        let associated_book = match resume {
            ResumePoint::Reader { book, .. } => Some(book),
            ResumePoint::Books { selected } => selected,
            ResumePoint::Files {
                selected: Some(file),
            } if file.index() < book_count => Some(BookId::new(file.index())),
            ResumePoint::Home { .. }
            | ResumePoint::Files { .. }
            | ResumePoint::Settings { .. }
            | ResumePoint::Image { .. } => None,
        };
        let custom = match selected_image {
            Some(image) => Self {
                sources: [
                    SleepScreenSource::CustomImage(image),
                    SleepScreenSource::BuiltIn,
                ],
                length: 2,
            },
            None => Self {
                sources: [SleepScreenSource::BuiltIn, SleepScreenSource::BuiltIn],
                length: 1,
            },
        };
        match mode {
            SleepScreenMode::Custom => custom,
            SleepScreenMode::BookCover => match associated_book {
                Some(book) => Self {
                    sources: [
                        SleepScreenSource::BookCover(book),
                        SleepScreenSource::BuiltIn,
                    ],
                    length: 2,
                },
                None => Self {
                    sources: [SleepScreenSource::BuiltIn, SleepScreenSource::BuiltIn],
                    length: 1,
                },
            },
            SleepScreenMode::Automatic => match resume {
                ResumePoint::Reader { book, .. } => Self {
                    sources: [
                        SleepScreenSource::BookCover(book),
                        SleepScreenSource::BuiltIn,
                    ],
                    length: 2,
                },
                _ => custom,
            },
        }
    }

    pub fn sources(self) -> impl Iterator<Item = SleepScreenSource> {
        self.sources.into_iter().take(self.length)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppEffect {
    None,
    Render,
    LoadChapter {
        book: BookId,
        spine_index: usize,
        target: PageTarget,
    },
    EnterDeepSleep {
        resume: ResumePoint,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppStateError {
    BookOutOfBounds,
    ImageOutOfBounds,
    SpineOutOfBounds,
    EmptyChapter,
    UnexpectedChapter,
    NotPreparingSleep,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingChapter {
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

fn battery_refresh_needed(previous: BatteryStatus, current: BatteryStatus) -> bool {
    if previous.usb() != current.usb() {
        return true;
    }
    match (previous.level(), current.level()) {
        (BatteryLevel::Unknown, BatteryLevel::Unknown) => false,
        (BatteryLevel::Percent(previous), BatteryLevel::Percent(current)) => {
            previous.get().abs_diff(current.get()) >= BATTERY_REFRESH_PERCENT_DELTA
        }
        _ => true,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BatteryDisplayState {
    current: BatteryStatus,
    refresh_anchor: BatteryStatus,
}

impl BatteryDisplayState {
    const fn new() -> Self {
        Self {
            current: BatteryStatus::unknown(),
            refresh_anchor: BatteryStatus::unknown(),
        }
    }

    fn update(&mut self, next: BatteryStatus) -> bool {
        if self.current == next {
            return false;
        }
        self.current = next;
        if !battery_refresh_needed(self.refresh_anchor, next) {
            return false;
        }
        self.refresh_anchor = next;
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct App {
    library: LibraryState,
    files: FilesState,
    home: HomeState,
    view: AppView,
    preferences: AppPreferences,
    image_count: usize,
    selected_sleep_image: Option<ImageId>,
    battery: BatteryDisplayState,
    reading_checkpoint: Option<ReadingCheckpoint>,
}

impl App {
    pub const fn new(book_count: usize) -> Self {
        Self::with_preferences(
            book_count,
            AppPreferences::new(
                ReaderPreferences::new(
                    ReaderFont::NotoSerif,
                    ReaderFontSize::Medium,
                    ReaderSpacing::Normal,
                ),
                SleepScreenMode::Automatic,
            ),
        )
    }

    pub const fn with_preferences(book_count: usize, preferences: AppPreferences) -> Self {
        Self::with_catalog(book_count, 0, None, preferences)
    }

    pub const fn with_catalog(
        book_count: usize,
        image_count: usize,
        selected_sleep_image: Option<ImageId>,
        preferences: AppPreferences,
    ) -> Self {
        let home = HomeState::new();
        let selected_sleep_image = match selected_sleep_image {
            Some(image) if image.index() < image_count => Some(image),
            _ if image_count > 0 => Some(ImageId::new(0)),
            _ => None,
        };
        Self {
            library: LibraryState::new(book_count),
            files: FilesState::new(book_count + image_count),
            home,
            view: AppView::Home(home),
            preferences,
            image_count,
            selected_sleep_image,
            battery: BatteryDisplayState::new(),
            reading_checkpoint: None,
        }
    }

    pub fn from_resume(
        book_count: usize,
        preferences: AppPreferences,
        resume: ResumePoint,
    ) -> Result<(Self, AppEffect), AppStateError> {
        Self::from_resume_with_catalog(book_count, 0, None, preferences, resume)
    }

    pub fn from_resume_with_catalog(
        book_count: usize,
        image_count: usize,
        selected_sleep_image: Option<ImageId>,
        preferences: AppPreferences,
        resume: ResumePoint,
    ) -> Result<(Self, AppEffect), AppStateError> {
        let mut app =
            Self::with_catalog(book_count, image_count, selected_sleep_image, preferences);
        let file_count = book_count + image_count;
        let effect = match resume {
            ResumePoint::Home { selected } => {
                app.home = HomeState::with_selected(selected);
                app.view = AppView::Home(app.home);
                AppEffect::Render
            }
            ResumePoint::Books { selected: None } if book_count == 0 => {
                app.view = AppView::Library;
                AppEffect::Render
            }
            ResumePoint::Books {
                selected: Some(selected),
            } => {
                app.library = LibraryState::with_selected(book_count, selected.index())
                    .map_err(|_| AppStateError::BookOutOfBounds)?;
                app.view = AppView::Library;
                AppEffect::Render
            }
            ResumePoint::Books { selected: None } => {
                app.view = AppView::Library;
                AppEffect::Render
            }
            ResumePoint::Files { selected: None } if file_count == 0 => {
                app.view = AppView::Files(app.files);
                AppEffect::Render
            }
            ResumePoint::Files {
                selected: Some(selected),
            } => {
                app.files = FilesState::with_selected(file_count, selected.index())
                    .map_err(|_| AppStateError::ImageOutOfBounds)?;
                app.view = AppView::Files(app.files);
                AppEffect::Render
            }
            ResumePoint::Files { selected: None } => {
                app.view = AppView::Files(app.files);
                AppEffect::Render
            }
            ResumePoint::Settings { selected, draft } => {
                let settings = SettingsState::with_state(selected, draft);
                app.home = HomeState::with_selected(HomeItem::Settings);
                app.view = AppView::Settings(settings);
                AppEffect::Render
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
                    preferences: preferences.reader(),
                });
                app.request_chapter(book, spine_index, PageTarget::Index(page_index), origin)
            }
            ResumePoint::Image { image } => {
                app.validate_image(image)?;
                app.view = AppView::Image(image);
                app.files.selected = Some(FileId::new(book_count + image.index()));
                AppEffect::Render
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

    pub const fn preferences(self) -> AppPreferences {
        self.preferences
    }

    pub const fn reader_preferences(self) -> ReaderPreferences {
        self.preferences.reader()
    }

    pub const fn image_count(self) -> usize {
        self.image_count
    }

    pub const fn selected_sleep_image(self) -> Option<ImageId> {
        self.selected_sleep_image
    }

    pub const fn sleep_screen_plan(self, resume: ResumePoint) -> SleepScreenPlan {
        SleepScreenPlan::resolve(
            self.preferences.sleep_screen(),
            resume,
            self.selected_sleep_image,
            self.library.book_count(),
        )
    }

    pub const fn battery(self) -> BatteryStatus {
        self.battery.current
    }

    pub fn set_battery(&mut self, battery: BatteryStatus) -> AppEffect {
        if !self.battery.update(battery) {
            return AppEffect::None;
        }
        self.current_render_effect()
    }

    fn current_render_effect(&self) -> AppEffect {
        match self.view {
            AppView::Loading(_) | AppView::Reader(_) | AppView::Sleeping { .. } => AppEffect::None,
            _ => AppEffect::Render,
        }
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
            AppView::Reader(session) | AppView::ReaderDrawer(ReaderDrawer { session, .. }) => {
                ResumePoint::Reader {
                    book: session.location.book,
                    spine_index: session.location.spine_index,
                    page_index: session.location.page_index,
                    origin: session.origin,
                }
            }
            AppView::Image(image) => ResumePoint::Image { image },
            AppView::Sleeping { resume } => resume,
            AppView::Loading(pending) => self.origin_resume(pending.origin),
            AppView::Error { origin, .. } => self.origin_resume(origin),
        }
    }

    pub fn input(&mut self, input: AppInput) -> AppEffect {
        if input == AppInput::Power && !matches!(self.view, AppView::Sleeping { .. }) {
            let resume = self.resume_point();
            self.view = AppView::Sleeping { resume };
            return AppEffect::Render;
        }

        match (self.view, input) {
            (AppView::Home(mut home), AppInput::Move(direction)) => {
                if !home.move_selection(direction) {
                    return AppEffect::None;
                }
                self.home = home;
                self.view = AppView::Home(home);
                AppEffect::Render
            }
            (AppView::Home(home), AppInput::Confirm) => match home.selected {
                HomeItem::Books => {
                    self.view = AppView::Library;
                    AppEffect::Render
                }
                HomeItem::Files => {
                    self.view = AppView::Files(self.files);
                    AppEffect::Render
                }
                HomeItem::Settings => {
                    self.view = AppView::Settings(SettingsState::new(self.preferences));
                    AppEffect::Render
                }
            },
            (AppView::Library, AppInput::Move(direction)) => {
                if self.library.move_selection(direction) {
                    AppEffect::Render
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
                AppEffect::Render
            }
            (AppView::Files(_), AppInput::Confirm) => self.open_file(),
            (AppView::Files(_), AppInput::Back) => self.return_home(HomeItem::Files),
            (AppView::Settings(mut settings), AppInput::Move(direction)) => {
                if !settings.input(direction) {
                    return AppEffect::None;
                }
                self.view = AppView::Settings(settings);
                AppEffect::Render
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
                AppEffect::Render
            }
            (AppView::Settings(_), AppInput::Back) => self.return_home(HomeItem::Settings),
            (AppView::Reader(session), AppInput::Move(Direction::Right | Direction::Down)) => {
                self.next_page(session)
            }
            (AppView::Reader(session), AppInput::Confirm) => {
                self.view =
                    AppView::ReaderDrawer(ReaderDrawer::new(session, self.reader_preferences()));
                AppEffect::Render
            }
            (AppView::ReaderDrawer(mut drawer), AppInput::Move(direction)) => {
                let previous = drawer;
                drawer.input(direction);
                if drawer == previous {
                    return AppEffect::None;
                }
                self.view = AppView::ReaderDrawer(drawer);
                AppEffect::Render
            }
            (AppView::ReaderDrawer(drawer), AppInput::Confirm) => self.apply_reader_drawer(drawer),
            (AppView::ReaderDrawer(drawer), AppInput::Back) => {
                self.view = AppView::Reader(drawer.session);
                AppEffect::Render
            }
            (AppView::Reader(session), AppInput::Move(Direction::Left | Direction::Up)) => {
                self.previous_page(session)
            }
            (AppView::Reader(session), AppInput::Back) => self.return_to_origin(session.origin),
            (AppView::Image(image), AppInput::Confirm) => {
                self.selected_sleep_image = Some(image);
                AppEffect::Render
            }
            (AppView::Image(_), AppInput::Back) => {
                self.view = AppView::Files(self.files);
                AppEffect::Render
            }
            (AppView::Loading(pending), AppInput::Back) => self.return_to_origin(pending.origin),
            (AppView::Error { origin, .. }, AppInput::Back) => self.return_to_origin(origin),
            _ => AppEffect::None,
        }
    }

    pub fn chapter_loaded(
        &mut self,
        spine_count: usize,
        page_count: usize,
    ) -> Result<AppEffect, AppStateError> {
        let AppView::Loading(pending) = self.view else {
            return Err(AppStateError::UnexpectedChapter);
        };
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
            preferences: self.preferences.reader(),
        });
        Ok(AppEffect::Render)
    }

    pub fn chapter_failed(&mut self) -> Result<AppEffect, AppStateError> {
        let AppView::Loading(pending) = self.view else {
            return Err(AppStateError::UnexpectedChapter);
        };
        self.view = AppView::Error {
            book: pending.book,
            origin: pending.origin,
        };
        Ok(AppEffect::Render)
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
                AppEffect::Render
            }
            ResumePoint::Books { selected } => {
                if let Some(selected) = selected {
                    self.select_book(selected);
                }
                self.view = AppView::Library;
                AppEffect::Render
            }
            ResumePoint::Files { selected } => {
                if let Some(selected) = selected
                    && let Ok(files) =
                        FilesState::with_selected(self.files.file_count, selected.index())
                {
                    self.files = files;
                }
                self.view = AppView::Files(self.files);
                AppEffect::Render
            }
            ResumePoint::Settings { selected, draft } => {
                let settings = SettingsState::with_state(selected, draft);
                self.view = AppView::Settings(settings);
                AppEffect::Render
            }
            ResumePoint::Reader {
                book,
                spine_index,
                page_index,
                origin,
            } => self.request_chapter(book, spine_index, PageTarget::Index(page_index), origin),
            ResumePoint::Image { image } => {
                self.view = AppView::Image(image);
                AppEffect::Render
            }
        }
    }

    fn open_selected(&mut self, origin: BookOrigin) -> AppEffect {
        self.library
            .selected
            .map_or(AppEffect::None, |book| self.open_book(book, origin))
    }

    fn open_file(&mut self) -> AppEffect {
        let Some(file) = self.files.selected else {
            return AppEffect::None;
        };
        if file.index() < self.library.book_count {
            return self.open_book(BookId::new(file.index()), BookOrigin::Files);
        }
        let image = ImageId::new(file.index() - self.library.book_count);
        self.view = AppView::Image(image);
        AppEffect::Render
    }

    fn open_book(&mut self, book: BookId, origin: BookOrigin) -> AppEffect {
        self.select_book(book);
        match self
            .reading_checkpoint
            .filter(|checkpoint| checkpoint.book == book)
        {
            Some(checkpoint) if checkpoint.preferences == self.preferences.reader() => self
                .request_chapter(
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

    fn apply_reader_drawer(&mut self, drawer: ReaderDrawer) -> AppEffect {
        let location = drawer.session.location;
        let typography_changed = drawer.preferences != self.reader_preferences();
        self.preferences = self.preferences.with_reader(drawer.preferences);
        if drawer.selected == ReaderControl::Chapter && drawer.chapter != location.spine_index {
            return self.request_chapter(
                location.book,
                drawer.chapter,
                PageTarget::First,
                drawer.session.origin,
            );
        }
        let page_index = if drawer.selected == ReaderControl::Page {
            drawer.page
        } else {
            location.page_index
        };
        if typography_changed {
            return self.request_chapter(
                location.book,
                location.spine_index,
                PageTarget::Progress {
                    page_index,
                    page_count: location.page_count,
                },
                drawer.session.origin,
            );
        }
        self.set_reading_session(
            ReadingLocation {
                page_index,
                ..location
            },
            drawer.session.origin,
        )
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
            preferences: self.preferences.reader(),
        });
        AppEffect::Render
    }

    fn request_chapter(
        &mut self,
        book: BookId,
        spine_index: usize,
        target: PageTarget,
        origin: BookOrigin,
    ) -> AppEffect {
        self.view = AppView::Loading(PendingChapter {
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
        AppEffect::Render
    }

    fn return_to_origin(&mut self, origin: BookOrigin) -> AppEffect {
        match origin {
            BookOrigin::Books => {
                self.view = AppView::Library;
                AppEffect::Render
            }
            BookOrigin::Files => {
                self.view = AppView::Files(self.files);
                AppEffect::Render
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
        if let Ok(files) = FilesState::with_selected(self.files.file_count, book.index()) {
            self.files = files;
        }
    }

    pub fn replace_image_catalog(
        &mut self,
        image_count: usize,
        selected_sleep_image: Option<ImageId>,
    ) -> AppEffect {
        self.image_count = image_count;
        self.selected_sleep_image = match selected_sleep_image {
            Some(image) if image.index() < image_count => Some(image),
            _ if image_count > 0 => Some(ImageId::new(0)),
            _ => None,
        };
        self.files
            .replace_count(self.library.book_count + self.image_count);
        if let AppView::Image(image) = self.view
            && image.index() >= image_count
        {
            self.view = AppView::Files(self.files);
        }
        self.current_render_effect()
    }

    fn validate_book(&self, book: BookId) -> Result<(), AppStateError> {
        if book.index() < self.library.book_count {
            Ok(())
        } else {
            Err(AppStateError::BookOutOfBounds)
        }
    }

    fn validate_image(&self, image: ImageId) -> Result<(), AppStateError> {
        if image.index() < self.image_count {
            Ok(())
        } else {
            Err(AppStateError::ImageOutOfBounds)
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
    extern crate std;

    use super::{
        App, AppEffect, AppInput, AppPreferences, AppView, BookOrigin, Direction, FilesState,
        HomeItem, ImageId, LibraryState, PageTarget, ReaderFont, ReaderFontSize, ReaderPreferences,
        ReaderSpacing, ResumePoint, SettingsItem, SleepScreenMode, SleepScreenPlan,
        SleepScreenSource,
    };
    use crate::{input::UsbState, power::BatteryStatus};

    fn open_books(app: &mut App) {
        assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
        assert_eq!(app.view(), AppView::Library);
    }

    fn open_first_book(app: &mut App, pages: usize) {
        open_books(app);
        assert!(matches!(
            app.input(AppInput::Confirm),
            AppEffect::LoadChapter { .. }
        ));
        assert_eq!(app.chapter_loaded(3, pages).unwrap(), AppEffect::Render);
        assert!(matches!(app.view(), AppView::Reader(_)));
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

        let preferences = AppPreferences::new(preferences, SleepScreenMode::BookCover);
        assert_eq!(
            AppPreferences::from_packed(preferences.packed()),
            Some(preferences)
        );
        assert_eq!(AppPreferences::from_packed(0x0300_0000), None);
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
        assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
        assert_eq!(app.view(), AppView::Library);
        assert_eq!(app.input(AppInput::Back), AppEffect::Render);
        assert_eq!(app.view(), AppView::Home(app.home()));

        app.input(AppInput::Move(Direction::Down));
        assert_eq!(app.home().selected(), HomeItem::Files);
        assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
        assert_eq!(app.view(), AppView::Files(FilesState::new(4)));
        assert_eq!(app.input(AppInput::Back), AppEffect::Render);
        assert_eq!(app.view(), AppView::Home(app.home()));

        app.input(AppInput::Move(Direction::Down));
        assert_eq!(app.home().selected(), HomeItem::Settings);
        assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
        assert!(matches!(app.view(), AppView::Settings(_)));
    }

    #[test]
    fn settings_use_a_draft_and_only_apply_from_the_apply_row() {
        let mut app = App::new(1);
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Confirm);
        app.input(AppInput::Move(Direction::Right));
        assert_eq!(app.preferences(), AppPreferences::default());
        app.input(AppInput::Back);
        assert_eq!(app.preferences(), AppPreferences::default());

        app.input(AppInput::Confirm);
        app.input(AppInput::Move(Direction::Right));
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Move(Direction::Right));
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Move(Direction::Right));
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Move(Direction::Down));
        assert!(matches!(
            app.view(),
            AppView::Settings(settings) if settings.selected() == SettingsItem::Apply
        ));
        assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
        assert_eq!(app.view(), AppView::Home(app.home()));
        assert_eq!(
            app.preferences(),
            AppPreferences::new(
                ReaderPreferences::new(
                    ReaderFont::Compact,
                    ReaderFontSize::Large,
                    ReaderSpacing::Relaxed,
                ),
                SleepScreenMode::Automatic,
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
        assert_eq!(app.input(AppInput::Back), AppEffect::Render);
        assert!(
            matches!(app.view(), AppView::Files(files) if files.selected().unwrap().index() == 1)
        );
    }

    #[test]
    fn files_open_images_and_confirm_selects_the_sleep_image() {
        let mut app = App::with_catalog(1, 2, Some(ImageId::new(1)), AppPreferences::default());
        app.input(AppInput::Move(Direction::Down));
        app.input(AppInput::Confirm);
        app.input(AppInput::Move(Direction::Down));
        assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
        assert_eq!(app.view(), AppView::Image(ImageId::new(0)));
        assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
        assert_eq!(app.view(), AppView::Image(ImageId::new(0)));
        assert_eq!(app.selected_sleep_image(), Some(ImageId::new(0)));
        assert_eq!(app.input(AppInput::Back), AppEffect::Render);
        assert_eq!(app.view(), AppView::Files(app.files()));
    }

    #[test]
    fn reader_turns_pages_and_crosses_chapter_boundaries() {
        let mut app = App::new(1);
        open_first_book(&mut app, 2);
        assert_eq!(
            app.input(AppInput::Move(Direction::Right)),
            AppEffect::Render
        );
        assert!(
            matches!(app.view(), AppView::Reader(session) if session.location().page_index() == 1)
        );
        assert!(matches!(
            app.input(AppInput::Move(Direction::Right)),
            AppEffect::LoadChapter {
                spine_index: 1,
                target: PageTarget::First,
                ..
            }
        ));
        assert_eq!(app.chapter_loaded(3, 4).unwrap(), AppEffect::Render);
        let AppView::Reader(session) = app.view() else {
            panic!("expected reader view");
        };
        let location = session.location();
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
        assert_eq!(app.chapter_loaded(3, 9).unwrap(), AppEffect::Render);
        let AppView::Reader(session) = app.view() else {
            panic!("expected reader view");
        };
        let location = session.location();
        assert_eq!(location.page_index(), 4);
    }

    #[test]
    fn sleep_and_wake_restore_settings_and_reader_origins() {
        let preferences = AppPreferences::new(
            ReaderPreferences::new(
                ReaderFont::Mono,
                ReaderFontSize::Large,
                ReaderSpacing::Compact,
            ),
            SleepScreenMode::Custom,
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
        assert_eq!(app.input(AppInput::Power), AppEffect::Render);
        assert!(matches!(
            app.view(),
            AppView::Sleeping {
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
        assert_eq!(app.input(AppInput::Back), AppEffect::Render);
        assert_eq!(app.view(), AppView::Files(app.files()));
        assert_eq!(app.preferences(), preferences);
    }

    #[test]
    fn sleep_screen_plans_are_context_sensitive_and_exhaustive() {
        let book = super::BookId::new(2);
        let image = ImageId::new(1);
        let reader = ResumePoint::Reader {
            book,
            spine_index: 0,
            page_index: 0,
            origin: BookOrigin::Books,
        };
        let home = ResumePoint::Home {
            selected: HomeItem::Books,
        };

        assert_eq!(
            SleepScreenPlan::resolve(SleepScreenMode::Automatic, reader, Some(image), 4)
                .sources()
                .collect::<std::vec::Vec<_>>(),
            std::vec![
                SleepScreenSource::BookCover(book),
                SleepScreenSource::BuiltIn
            ]
        );
        assert_eq!(
            SleepScreenPlan::resolve(SleepScreenMode::Automatic, home, Some(image), 4)
                .sources()
                .collect::<std::vec::Vec<_>>(),
            std::vec![
                SleepScreenSource::CustomImage(image),
                SleepScreenSource::BuiltIn
            ]
        );
        assert_eq!(
            SleepScreenPlan::resolve(SleepScreenMode::Custom, reader, Some(image), 4)
                .sources()
                .collect::<std::vec::Vec<_>>(),
            std::vec![
                SleepScreenSource::CustomImage(image),
                SleepScreenSource::BuiltIn
            ]
        );
        assert_eq!(
            SleepScreenPlan::resolve(
                SleepScreenMode::BookCover,
                ResumePoint::Books {
                    selected: Some(book)
                },
                Some(image),
                4,
            )
            .sources()
            .collect::<std::vec::Vec<_>>(),
            std::vec![
                SleepScreenSource::BookCover(book),
                SleepScreenSource::BuiltIn
            ]
        );
        assert_eq!(
            SleepScreenPlan::resolve(SleepScreenMode::BookCover, home, Some(image), 4)
                .sources()
                .collect::<std::vec::Vec<_>>(),
            std::vec![SleepScreenSource::BuiltIn]
        );
    }

    #[test]
    fn rejected_chapter_metadata_preserves_the_loading_request() {
        let mut app = App::new(1);
        open_books(&mut app);
        app.input(AppInput::Confirm);
        let loading = app.view();
        assert!(matches!(loading, AppView::Loading(_)));
        assert_eq!(
            app.set_battery(BatteryStatus::from_percent(82, UsbState::Disconnected)),
            AppEffect::None,
        );
        assert_eq!(app.view(), loading);
        assert_eq!(
            app.chapter_loaded(0, 1),
            Err(super::AppStateError::SpineOutOfBounds)
        );
        assert_eq!(app.view(), loading);
        assert_eq!(
            app.chapter_loaded(1, 0),
            Err(super::AppStateError::EmptyChapter)
        );
        assert_eq!(app.view(), loading);
        app.chapter_loaded(1, 2).unwrap();
        assert!(matches!(app.view(), AppView::Reader(_)));
        assert_eq!(
            app.chapter_failed(),
            Err(super::AppStateError::UnexpectedChapter)
        );
    }

    #[test]
    fn cancelling_or_sleeping_drops_the_loading_request_and_keeps_the_origin() {
        for origin in [BookOrigin::Books, BookOrigin::Files] {
            for interrupt in [AppInput::Back, AppInput::Power] {
                let mut app = App::new(1);
                if origin == BookOrigin::Files {
                    app.input(AppInput::Move(Direction::Down));
                }
                app.input(AppInput::Confirm);
                let parent = app.view();
                let resume = app.resume_point();
                app.input(AppInput::Confirm);
                assert!(matches!(app.view(), AppView::Loading(_)));
                assert_eq!(app.resume_point(), resume);
                app.input(interrupt);
                assert_eq!(
                    app.chapter_loaded(1, 1),
                    Err(super::AppStateError::UnexpectedChapter)
                );
                assert_eq!(
                    app.chapter_failed(),
                    Err(super::AppStateError::UnexpectedChapter)
                );
                if interrupt == AppInput::Power {
                    assert!(matches!(app.view(), AppView::Sleeping { .. }));
                    app.wake();
                }
                assert_eq!(app.view(), parent);
            }
        }
    }

    #[test]
    fn settings_metadata_matches_the_navigation_order() {
        let preferences = AppPreferences::default();
        for (index, item) in SettingsItem::ALL.into_iter().enumerate() {
            assert_eq!(item.index(), index);
            assert_eq!(SettingsItem::from_index(index), Some(item));
            assert_eq!(
                item.value(preferences).is_none(),
                item == SettingsItem::Apply
            );
        }
        assert_eq!(SettingsItem::from_index(SettingsItem::ALL.len()), None);
        assert_eq!(SettingsItem::from_index(usize::MAX), None);
    }

    #[test]
    fn battery_updates_refresh_meaningful_changes() {
        let mut app = App::new(0);
        let initial = BatteryStatus::from_percent(42, UsbState::Disconnected);
        assert_eq!(app.set_battery(initial), AppEffect::Render);
        assert_eq!(app.set_battery(initial), AppEffect::None);

        let small_change = BatteryStatus::from_percent(44, UsbState::Disconnected);
        assert_eq!(app.set_battery(small_change), AppEffect::None);
        assert_eq!(app.battery(), small_change);

        let threshold_change = BatteryStatus::from_percent(47, UsbState::Disconnected);
        assert_eq!(app.set_battery(threshold_change), AppEffect::Render);

        let usb_connected = BatteryStatus::from_percent(47, UsbState::Connected);
        assert_eq!(app.set_battery(usb_connected), AppEffect::Render);
        assert_eq!(app.battery(), usb_connected);
    }
}
