use super::{BookId, Direction, SelectionOutOfBounds};

pub const MAX_LIBRARY_BOOKS: usize = 16;
pub const LIBRARY_ROWS_PER_PAGE: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SeriesPosition(u16);

impl SeriesPosition {
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (integer, fraction) = match text.split_once('.') {
            Some((integer, fraction)) => (integer, Some(fraction)),
            None => (text, None),
        };
        if integer.is_empty() || !integer.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        if fraction.is_some_and(|fraction| {
            fraction.is_empty() || !fraction.bytes().all(|byte| byte == b'0')
        }) {
            return None;
        }
        Some(Self(integer.parse().ok()?))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeriesMeta<'a> {
    name: &'a str,
    position: Option<SeriesPosition>,
}

impl<'a> SeriesMeta<'a> {
    pub const fn new(name: &'a str, position: Option<SeriesPosition>) -> Self {
        Self { name, position }
    }

    pub const fn name(self) -> &'a str {
        self.name
    }

    pub const fn position(self) -> Option<SeriesPosition> {
        self.position
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BookMeta<'a> {
    title: &'a str,
    creator: &'a str,
    series: Option<SeriesMeta<'a>>,
}

impl<'a> BookMeta<'a> {
    pub const fn empty() -> Self {
        Self {
            title: "",
            creator: "",
            series: None,
        }
    }

    pub const fn new(title: &'a str, creator: &'a str, series: Option<SeriesMeta<'a>>) -> Self {
        Self {
            title,
            creator,
            series,
        }
    }

    pub const fn title(self) -> &'a str {
        self.title
    }

    pub const fn creator(self) -> &'a str {
        self.creator
    }

    pub const fn series(self) -> Option<SeriesMeta<'a>> {
        self.series
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CatalogSlot(u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RowCursor(u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MemberRange {
    start: u8,
    length: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RootRow {
    Book {
        book: CatalogSlot,
    },
    Series {
        anchor: CatalogSlot,
        members: MemberRange,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogTooLarge;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibraryIndex {
    roots: [Option<RootRow>; MAX_LIBRARY_BOOKS],
    root_count: u8,
    members: [CatalogSlot; MAX_LIBRARY_BOOKS],
    member_count: u8,
    book_count: u8,
    catalog_key: u32,
}

impl LibraryIndex {
    pub const fn book_count(&self) -> usize {
        self.book_count as usize
    }

    pub fn flat(book_count: usize) -> Result<Self, CatalogTooLarge> {
        let books = [BookMeta::empty(); MAX_LIBRARY_BOOKS];
        Self::try_from(books.get(..book_count).ok_or(CatalogTooLarge)?)
    }

    pub fn root_containing(&self, book: usize) -> Option<usize> {
        for (index, root) in self.roots[..usize::from(self.root_count)]
            .iter()
            .enumerate()
        {
            let Some(root) = root else {
                continue;
            };
            match *root {
                RootRow::Book { book: slot } if usize::from(slot.0) == book => return Some(index),
                RootRow::Series { members, .. } => {
                    let range =
                        usize::from(members.start)..usize::from(members.start + members.length);
                    if self.members[range]
                        .iter()
                        .any(|slot| usize::from(slot.0) == book)
                    {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn push_root(&mut self, row: RootRow) {
        self.roots[usize::from(self.root_count)] = Some(row);
        self.root_count += 1;
    }

    fn derive(&mut self, books: &[BookMeta<'_>]) {
        let mut grouped = [false; MAX_LIBRARY_BOOKS];
        for (slot, book) in books.iter().enumerate() {
            if grouped[slot] {
                continue;
            }
            let Some(series) = declared_series(*book) else {
                self.push_root(RootRow::Book {
                    book: CatalogSlot(slot as u8),
                });
                continue;
            };
            let members = books
                .iter()
                .enumerate()
                .filter(|(other, candidate)| {
                    !grouped[*other]
                        && declared_series(**candidate)
                            .is_some_and(|candidate| candidate.name == series.name)
                })
                .count();
            if members < 2 {
                self.push_root(RootRow::Book {
                    book: CatalogSlot(slot as u8),
                });
                continue;
            }
            let start = self.member_count;
            for (other, candidate) in books.iter().enumerate() {
                if !grouped[other]
                    && declared_series(*candidate)
                        .is_some_and(|candidate| candidate.name == series.name)
                {
                    grouped[other] = true;
                    self.members[usize::from(self.member_count)] = CatalogSlot(other as u8);
                    self.member_count += 1;
                }
            }
            let range = usize::from(start)..usize::from(self.member_count);
            let key = |slot: CatalogSlot| {
                let series = books[usize::from(slot.0)]
                    .series
                    .expect("grouped books declare a series");
                (
                    series.position.is_none(),
                    series.position.map_or(0, SeriesPosition::value),
                    slot.0,
                )
            };
            for index in 1..range.len() {
                let mut position = index;
                while position > 0
                    && key(self.members[range.start + position - 1])
                        > key(self.members[range.start + position])
                {
                    self.members
                        .swap(range.start + position - 1, range.start + position);
                    position -= 1;
                }
            }
            self.push_root(RootRow::Series {
                anchor: CatalogSlot(slot as u8),
                members: MemberRange {
                    start,
                    length: self.member_count - start,
                },
            });
        }
    }
}

impl TryFrom<&[BookMeta<'_>]> for LibraryIndex {
    type Error = CatalogTooLarge;

    fn try_from(books: &[BookMeta<'_>]) -> Result<Self, Self::Error> {
        if books.len() > MAX_LIBRARY_BOOKS {
            return Err(CatalogTooLarge);
        }
        let mut index = Self {
            roots: [None; MAX_LIBRARY_BOOKS],
            root_count: 0,
            members: [CatalogSlot(0); MAX_LIBRARY_BOOKS],
            member_count: 0,
            book_count: books.len() as u8,
            catalog_key: catalog_key(books),
        };
        index.derive(books);
        Ok(index)
    }
}

fn declared_series(book: BookMeta<'_>) -> Option<SeriesMeta<'_>> {
    book.series.filter(|series| !series.name().is_empty())
}

fn catalog_key(books: &[BookMeta<'_>]) -> u32 {
    let mut hash = 0x811C_9DC5u32;
    let mut write = |bytes: &[u8]| {
        for byte in bytes {
            hash = (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
        }
    };
    write(&(books.len() as u32).to_le_bytes());
    for book in books {
        write(book.title.as_bytes());
        write(&[0]);
        write(book.creator.as_bytes());
        write(&[0]);
        match book.series {
            Some(series) => {
                write(series.name.as_bytes());
                write(&[0]);
                match series.position {
                    Some(position) => {
                        write(&[1]);
                        write(&position.value().to_le_bytes());
                    }
                    None => write(&[0]),
                }
            }
            None => write(&[0xFF, 0xFF]),
        }
    }
    hash
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cursor {
    Empty,
    Root(RowCursor),
    Series {
        parent: RowCursor,
        selected: RowCursor,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryEntry {
    Book(BookId),
    Series { anchor: BookId, volumes: u8 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryActivation {
    EnteredSeries,
    Open(BookId),
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryBack {
    Root,
    Home,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogMismatch {
    Length { expected: usize, actual: usize },
    CatalogChanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibraryState {
    index: LibraryIndex,
    cursor: Cursor,
}

impl LibraryState {
    pub const fn new(index: LibraryIndex) -> Self {
        Self {
            cursor: if index.root_count == 0 {
                Cursor::Empty
            } else {
                Cursor::Root(RowCursor(0))
            },
            index,
        }
    }

    pub fn replace(&mut self, index: LibraryIndex, anchor: Option<BookId>) {
        self.index = index;
        if self.restore_root(anchor).is_err() {
            self.cursor = if self.index.root_count == 0 {
                Cursor::Empty
            } else {
                Cursor::Root(RowCursor(0))
            };
        }
    }

    pub const fn book_count(&self) -> usize {
        self.index.book_count as usize
    }

    pub const fn row_count(&self) -> usize {
        match self.cursor {
            Cursor::Empty => 0,
            Cursor::Root(_) => self.index.root_count as usize,
            Cursor::Series { parent, .. } => match self.index.roots[parent.0 as usize] {
                Some(RootRow::Series { members, .. }) => members.length as usize,
                _ => 0,
            },
        }
    }

    pub const fn in_series(&self) -> bool {
        matches!(self.cursor, Cursor::Series { .. })
    }

    pub const fn row(&self) -> Option<usize> {
        match self.cursor {
            Cursor::Empty => None,
            Cursor::Root(row) => Some(row.0 as usize),
            Cursor::Series { selected, .. } => Some(selected.0 as usize),
        }
    }

    pub fn selected(&self) -> Option<LibraryEntry> {
        let row = self.row()?;
        match self.cursor {
            Cursor::Empty => None,
            Cursor::Root(_) => match self.index.roots[row]? {
                RootRow::Book { book } => {
                    Some(LibraryEntry::Book(BookId::new(usize::from(book.0))))
                }
                RootRow::Series { anchor, members } => Some(LibraryEntry::Series {
                    anchor: BookId::new(usize::from(anchor.0)),
                    volumes: members.length,
                }),
            },
            Cursor::Series { parent, .. } => {
                let Some(RootRow::Series { members, .. }) = self.index.roots[usize::from(parent.0)]
                else {
                    return None;
                };
                let slot = self.index.members[usize::from(members.start) + row];
                Some(LibraryEntry::Book(BookId::new(usize::from(slot.0))))
            }
        }
    }

    pub fn selected_book(&self) -> Option<BookId> {
        match self.selected()? {
            LibraryEntry::Book(book) => Some(book),
            LibraryEntry::Series { anchor, .. } => Some(anchor),
        }
    }

    pub fn scope_name<'a>(&self, books: &'a [BookMeta<'a>]) -> Option<&'a str> {
        let Cursor::Series { parent, .. } = self.cursor else {
            return None;
        };
        let Some(RootRow::Series { anchor, .. }) = self.index.roots[usize::from(parent.0)] else {
            return None;
        };
        books
            .get(usize::from(anchor.0))?
            .series()
            .map(SeriesMeta::name)
    }

    pub const fn page(&self) -> usize {
        match self.row() {
            Some(row) => row / LIBRARY_ROWS_PER_PAGE,
            None => 0,
        }
    }

    pub const fn page_count(&self) -> usize {
        self.row_count().div_ceil(LIBRARY_ROWS_PER_PAGE)
    }

    pub fn visible_range(&self) -> core::ops::Range<usize> {
        let start = self.page() * LIBRARY_ROWS_PER_PAGE;
        start..(start + LIBRARY_ROWS_PER_PAGE).min(self.row_count())
    }

    pub fn move_selection(&mut self, direction: Direction) -> bool {
        let Some(row) = self.row() else {
            return false;
        };
        let rows = self.row_count();
        let next = match direction {
            Direction::Up => row.saturating_sub(1),
            Direction::Down => (row + 1).min(rows - 1),
            Direction::Left => row.saturating_sub(LIBRARY_ROWS_PER_PAGE),
            Direction::Right => (row + LIBRARY_ROWS_PER_PAGE).min(rows - 1),
        };
        if next == row {
            return false;
        }
        match &mut self.cursor {
            Cursor::Empty => return false,
            Cursor::Root(cursor) => cursor.0 = next as u8,
            Cursor::Series { selected, .. } => selected.0 = next as u8,
        }
        true
    }

    pub fn activate(&mut self) -> LibraryActivation {
        let Some(row) = self.row() else {
            return LibraryActivation::None;
        };
        match self.cursor {
            Cursor::Empty => LibraryActivation::None,
            Cursor::Root(_) => match self.index.roots[row] {
                Some(RootRow::Book { book }) => {
                    LibraryActivation::Open(BookId::new(usize::from(book.0)))
                }
                Some(RootRow::Series { .. }) => {
                    self.cursor = Cursor::Series {
                        parent: RowCursor(row as u8),
                        selected: RowCursor(0),
                    };
                    LibraryActivation::EnteredSeries
                }
                None => LibraryActivation::None,
            },
            Cursor::Series { .. } => match self.selected() {
                Some(LibraryEntry::Book(book)) => LibraryActivation::Open(book),
                _ => LibraryActivation::None,
            },
        }
    }

    pub fn back(&mut self) -> LibraryBack {
        match self.cursor {
            Cursor::Series { parent, .. } => {
                self.cursor = Cursor::Root(parent);
                LibraryBack::Root
            }
            _ => LibraryBack::Home,
        }
    }

    pub fn select_book(&mut self, book: BookId) -> Result<(), SelectionOutOfBounds> {
        let slot = book.index();
        if slot >= self.book_count() {
            return Err(SelectionOutOfBounds);
        }
        let mut target = None;
        for (root_index, root) in self.index.roots[..usize::from(self.index.root_count)]
            .iter()
            .enumerate()
        {
            let Some(root) = root else {
                continue;
            };
            match *root {
                RootRow::Book { book: row_book } if usize::from(row_book.0) == slot => {
                    target = Some(Cursor::Root(RowCursor(root_index as u8)));
                }
                RootRow::Series { members, .. } => {
                    let start = usize::from(members.start);
                    let end = start + usize::from(members.length);
                    if let Some(offset) = self.index.members[start..end]
                        .iter()
                        .position(|member| usize::from(member.0) == slot)
                    {
                        target = Some(Cursor::Series {
                            parent: RowCursor(root_index as u8),
                            selected: RowCursor(offset as u8),
                        });
                    }
                }
                _ => {}
            }
            if target.is_some() {
                break;
            }
        }
        self.cursor = target.ok_or(SelectionOutOfBounds)?;
        Ok(())
    }

    pub fn restore_root(&mut self, book: Option<BookId>) -> Result<(), SelectionOutOfBounds> {
        match book {
            Some(book) => {
                let root = self
                    .index
                    .root_containing(book.index())
                    .ok_or(SelectionOutOfBounds)?;
                self.cursor = Cursor::Root(RowCursor(root as u8));
            }
            None => {
                self.cursor = if self.index.root_count == 0 {
                    Cursor::Empty
                } else {
                    Cursor::Root(RowCursor(0))
                };
            }
        }
        Ok(())
    }

    pub fn view<'a>(
        &'a self,
        books: &'a [BookMeta<'a>],
    ) -> Result<LibraryView<'a>, CatalogMismatch> {
        let expected = self.book_count();
        if books.len() != expected {
            return Err(CatalogMismatch::Length {
                expected,
                actual: books.len(),
            });
        }
        if catalog_key(books) != self.index.catalog_key {
            return Err(CatalogMismatch::CatalogChanged);
        }
        Ok(LibraryView { state: self, books })
    }

    fn library_row<'a>(&self, books: &'a [BookMeta<'a>], row: usize) -> Option<LibraryRow<'a>> {
        match self.cursor {
            Cursor::Empty => None,
            Cursor::Root(_) => match self.index.roots.get(row)?.as_ref()? {
                RootRow::Book { book } => {
                    let meta = books.get(usize::from(book.0))?;
                    Some(LibraryRow::Book {
                        id: BookId::new(usize::from(book.0)),
                        title: meta.title(),
                        creator: meta.creator(),
                    })
                }
                RootRow::Series { anchor, members } => {
                    let meta = books.get(usize::from(anchor.0))?;
                    let series = meta.series()?;
                    Some(LibraryRow::Series {
                        name: series.name(),
                        volumes: members.length,
                    })
                }
            },
            Cursor::Series { parent, .. } => {
                let Some(RootRow::Series { members, .. }) = self
                    .index
                    .roots
                    .get(usize::from(parent.0))
                    .and_then(|entry| entry.as_ref())
                else {
                    return None;
                };
                let slot = self.index.members.get(usize::from(members.start) + row)?;
                let meta = books.get(usize::from(slot.0))?;
                Some(LibraryRow::Book {
                    id: BookId::new(usize::from(slot.0)),
                    title: meta.title(),
                    creator: meta.creator(),
                })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryRow<'a> {
    Book {
        id: BookId,
        title: &'a str,
        creator: &'a str,
    },
    Series {
        name: &'a str,
        volumes: u8,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisibleLibraryRow<'a> {
    pub row: LibraryRow<'a>,
    pub selected: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibraryView<'a> {
    state: &'a LibraryState,
    books: &'a [BookMeta<'a>],
}

impl<'a> LibraryView<'a> {
    pub fn visible_rows(&self) -> impl Iterator<Item = VisibleLibraryRow<'a>> + '_ {
        VisibleRows {
            state: self.state,
            books: self.books,
            range: self.state.visible_range(),
        }
    }

    pub fn selected_row(&self) -> Option<LibraryRow<'a>> {
        let row = self.state.row()?;
        self.state.library_row(self.books, row)
    }

    pub fn scope_name(&self) -> Option<&'a str> {
        self.state.scope_name(self.books)
    }

    pub const fn page(&self) -> usize {
        self.state.page()
    }

    pub const fn page_count(&self) -> usize {
        self.state.page_count()
    }

    pub const fn row_count(&self) -> usize {
        self.state.row_count()
    }
}

struct VisibleRows<'a> {
    state: &'a LibraryState,
    books: &'a [BookMeta<'a>],
    range: core::ops::Range<usize>,
}

impl<'a> Iterator for VisibleRows<'a> {
    type Item = VisibleLibraryRow<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let row = self.range.next()?;
        let value = self.state.library_row(self.books, row)?;
        Some(VisibleLibraryRow {
            row: value,
            selected: self.state.row() == Some(row),
        })
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec::Vec;

    use super::*;

    fn series(name: &str, position: Option<u16>) -> Option<SeriesMeta<'_>> {
        Some(SeriesMeta::new(name, position.map(SeriesPosition::new)))
    }

    fn row_ids(state: &LibraryState, books: &[BookMeta<'_>]) -> Vec<BookId> {
        state
            .view(books)
            .unwrap()
            .visible_rows()
            .map(|visible| match visible.row {
                LibraryRow::Book { id, .. } => id,
                LibraryRow::Series { .. } => panic!("expected a book row"),
            })
            .collect()
    }

    #[test]
    fn series_positions_accept_decimals_and_reject_other_forms() {
        for (text, expected) in [
            ("0", 0),
            ("2", 2),
            (" 2 ", 2),
            ("1.00", 1),
            ("007", 7),
            ("65535", 65535),
        ] {
            assert_eq!(
                SeriesPosition::parse(text),
                Some(SeriesPosition::new(expected)),
                "{text}"
            );
        }
        for text in [
            "", " ", "1.5", "1.", ".5", "-1", "+2", "abc", "65536", "1 2", "1e2", "１２",
        ] {
            assert_eq!(SeriesPosition::parse(text), None, "{text}");
        }
    }

    #[test]
    fn folders_take_the_first_member_position_and_sort_their_volumes() {
        let books = [
            BookMeta::new("A", "X", series("S", Some(3))),
            BookMeta::new("B", "X", series("S", None)),
            BookMeta::new("C", "X", None),
            BookMeta::new("D", "X", series("S", Some(1))),
            BookMeta::new("E", "X", series("O", Some(1))),
            BookMeta::new("F", "X", series("O", Some(2))),
        ];
        let index = LibraryIndex::try_from(&books[..]).unwrap();
        let mut state = LibraryState::new(index);

        assert_eq!(state.book_count(), 6);
        assert_eq!(state.row_count(), 3);
        assert_eq!(
            state.selected(),
            Some(LibraryEntry::Series {
                anchor: BookId::new(0),
                volumes: 3,
            })
        );
        assert_eq!(state.selected_book(), Some(BookId::new(0)));
        assert_eq!(state.activate(), LibraryActivation::EnteredSeries);
        assert_eq!(state.row_count(), 3);
        assert_eq!(state.selected(), Some(LibraryEntry::Book(BookId::new(3))));
        assert_eq!(row_ids(&state, &books), [3, 0, 1].map(BookId::new).to_vec());
        assert_eq!(state.back(), LibraryBack::Root);
        assert_eq!(
            state.selected(),
            Some(LibraryEntry::Series {
                anchor: BookId::new(0),
                volumes: 3,
            })
        );
        assert!(state.move_selection(Direction::Down));
        assert_eq!(state.selected(), Some(LibraryEntry::Book(BookId::new(2))));
        assert!(state.move_selection(Direction::Down));
        assert_eq!(
            state.selected(),
            Some(LibraryEntry::Series {
                anchor: BookId::new(4),
                volumes: 2,
            })
        );
    }

    #[test]
    fn singleton_series_books_keep_ordinary_rows() {
        let books = [BookMeta::new("A", "X", series("S", Some(1)))];
        let state = LibraryState::new(LibraryIndex::try_from(&books[..]).unwrap());
        assert_eq!(state.row_count(), 1);
        assert_eq!(state.selected(), Some(LibraryEntry::Book(BookId::new(0))));
    }

    #[test]
    fn ties_and_absent_positions_keep_catalog_order() {
        let books = [
            BookMeta::new("A", "X", series("S", Some(2))),
            BookMeta::new("B", "X", series("S", Some(2))),
            BookMeta::new("C", "X", series("S", None)),
            BookMeta::new("D", "X", series("S", None)),
        ];
        let mut state = LibraryState::new(LibraryIndex::try_from(&books[..]).unwrap());
        assert_eq!(state.row_count(), 1);
        state.activate();
        assert_eq!(
            row_ids(&state, &books),
            [0, 1, 2, 3].map(BookId::new).to_vec()
        );
    }

    #[test]
    fn capacity_covers_flat_and_grouped_catalogs() {
        let flat = [BookMeta::empty(); MAX_LIBRARY_BOOKS];
        assert_eq!(
            LibraryIndex::try_from(&flat[..]).unwrap().book_count(),
            MAX_LIBRARY_BOOKS
        );
        let mut oversized = [BookMeta::empty(); MAX_LIBRARY_BOOKS + 1];
        assert_eq!(LibraryIndex::try_from(&oversized[..]), Err(CatalogTooLarge));

        for book in oversized.iter_mut() {
            *book = BookMeta::new("Book", "Author", series("Series", Some(1)));
        }
        let grouped = LibraryIndex::try_from(&oversized[..MAX_LIBRARY_BOOKS]).unwrap();
        let state = LibraryState::new(grouped);
        assert_eq!(state.book_count(), MAX_LIBRARY_BOOKS);
        assert_eq!(state.row_count(), 1);
        assert_eq!(
            state.selected(),
            Some(LibraryEntry::Series {
                anchor: BookId::new(0),
                volumes: MAX_LIBRARY_BOOKS as u8,
            })
        );
    }

    #[test]
    fn cursor_moves_by_one_row_and_pages_by_eight() {
        let books = [BookMeta::empty(); 10];
        let index = LibraryIndex::try_from(&books[..]).unwrap();
        let mut state = LibraryState::new(index);

        assert_eq!(state.row(), Some(0));
        assert_eq!(state.page(), 0);
        assert!(!state.move_selection(Direction::Up));
        assert!(!state.move_selection(Direction::Left));
        for _ in 0..7 {
            assert!(state.move_selection(Direction::Down));
        }
        assert_eq!(state.row(), Some(7));
        assert_eq!(state.page(), 0);
        assert!(state.move_selection(Direction::Down));
        assert_eq!(state.row(), Some(8));
        assert_eq!(state.page(), 1);
        assert_eq!(state.visible_range(), 8..10);
        assert!(state.move_selection(Direction::Right));
        assert_eq!(state.row(), Some(9));
        assert!(!state.move_selection(Direction::Right));
        assert!(state.move_selection(Direction::Left));
        assert_eq!(state.row(), Some(1));
        assert_eq!(state.page(), 0);

        let mut empty = LibraryState::new(LibraryIndex::flat(0).unwrap());
        assert_eq!(empty.selected(), None);
        assert_eq!(empty.row(), None);
        assert_eq!(empty.row_count(), 0);
        assert_eq!(empty.page_count(), 0);
        assert_eq!(empty.activate(), LibraryActivation::None);
        assert_eq!(empty.back(), LibraryBack::Home);
    }

    #[test]
    fn selecting_a_book_enters_its_folder_and_restores_the_root() {
        let books = [
            BookMeta::new("A", "X", series("S", Some(1))),
            BookMeta::new("B", "X", series("S", Some(2))),
            BookMeta::new("C", "X", None),
        ];
        let index = LibraryIndex::try_from(&books[..]).unwrap();
        let mut state = LibraryState::new(index);

        assert!(state.select_book(BookId::new(1)).is_ok());
        assert_eq!(state.selected_book(), Some(BookId::new(1)));
        assert_eq!(state.row_count(), 2);
        assert!(state.select_book(BookId::new(1)).is_ok());
        assert_eq!(state.selected_book(), Some(BookId::new(1)));
        assert!(state.select_book(BookId::new(2)).is_ok());
        assert_eq!(state.selected(), Some(LibraryEntry::Book(BookId::new(2))));
        assert_eq!(state.select_book(BookId::new(9)), Err(SelectionOutOfBounds));

        assert!(state.restore_root(Some(BookId::new(1))).is_ok());
        assert_eq!(
            state.selected(),
            Some(LibraryEntry::Series {
                anchor: BookId::new(0),
                volumes: 2,
            })
        );
        assert!(state.restore_root(None).is_ok());
        assert_eq!(state.row(), Some(0));
        assert_eq!(
            state.restore_root(Some(BookId::new(9))),
            Err(SelectionOutOfBounds)
        );
    }

    #[test]
    fn replace_restores_the_root_containing_the_anchor() {
        let books = [
            BookMeta::new("A", "X", series("S", Some(1))),
            BookMeta::new("B", "X", series("S", Some(2))),
            BookMeta::new("C", "X", None),
        ];
        let index = LibraryIndex::try_from(&books[..]).unwrap();
        let mut state = LibraryState::new(index);
        state.select_book(BookId::new(1)).unwrap();

        state.replace(index, Some(BookId::new(1)));
        assert_eq!(state.row_count(), 2);
        assert_eq!(
            state.selected(),
            Some(LibraryEntry::Series {
                anchor: BookId::new(0),
                volumes: 2,
            })
        );

        state.replace(index, None);
        assert_eq!(state.row(), Some(0));
        assert_eq!(
            state.selected(),
            Some(LibraryEntry::Series {
                anchor: BookId::new(0),
                volumes: 2,
            })
        );

        state.replace(index, Some(BookId::new(9)));
        assert_eq!(state.row(), Some(0));
    }

    #[test]
    fn view_rejects_a_catalog_that_does_not_match_the_index() {
        let books = [BookMeta::new("A", "X", None), BookMeta::new("B", "X", None)];
        let index = LibraryIndex::try_from(&books[..]).unwrap();
        let state = LibraryState::new(index);

        assert_eq!(
            state.view(&books[..1]),
            Err(CatalogMismatch::Length {
                expected: 2,
                actual: 1,
            })
        );
        let changed = [BookMeta::new("A", "X", None), BookMeta::new("B", "Y", None)];
        assert_eq!(
            state.view(&changed[..]),
            Err(CatalogMismatch::CatalogChanged)
        );
        let reordered = [books[1], books[0]];
        assert_eq!(
            state.view(&reordered[..]),
            Err(CatalogMismatch::CatalogChanged)
        );
        assert!(state.view(&books[..]).is_ok());

        let positioned_books = [
            BookMeta::new("A", "X", series("S", Some(65535))),
            BookMeta::new("B", "X", series("S", Some(2))),
        ];
        let positioned = LibraryState::new(LibraryIndex::try_from(&positioned_books[..]).unwrap());
        let repositioned = [
            BookMeta::new("A", "X", series("S", None)),
            positioned_books[1],
        ];
        assert_eq!(
            positioned.view(&repositioned[..]),
            Err(CatalogMismatch::CatalogChanged)
        );
    }
}
