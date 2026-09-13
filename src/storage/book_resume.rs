use crate::app::{
    AppPreferences, BookId, BookOrigin, FileId, HomeItem, ImageId, ResumePoint, SettingsItem,
};

use super::catalog::{BookFile, BookFileName, MAX_BOOK_NAME_BYTES};

pub const RESUME_WORDS: usize = 10 + MAX_BOOK_NAME_BYTES.div_ceil(4);
const LEGACY_MAGIC: u32 = 0x4257_5233;
const RESUME_MAGIC: u32 = 0x4257_5234;
const HOME_KIND: u32 = 1;
const BOOKS_KIND: u32 = 2;
const FILES_KIND: u32 = 3;
const SETTINGS_KIND: u32 = 4;
const READER_KIND: u32 = 5;
const IMAGE_KIND: u32 = 6;
const FILES_BOOK_KIND: u32 = 7;
const FILES_IMAGE_KIND: u32 = 8;
const CHECKSUM_WORD: usize = 7;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BookIdentity {
    name: BookFileName,
    size: u32,
}

impl From<&BookFile> for BookIdentity {
    fn from(file: &BookFile) -> Self {
        Self {
            name: *file.name(),
            size: file.size(),
        }
    }
}

impl BookIdentity {
    pub fn resolve(self, books: &[Option<BookFile>]) -> Result<BookId, ResumeError> {
        let mut matched = None;
        for (index, file) in books.iter().enumerate() {
            let Some(file) = file else { continue };
            if file.name() == &self.name && file.size() == self.size {
                if matched.is_some() {
                    return Err(ResumeError::Ambiguous);
                }
                matched = Some(BookId::new(index));
            }
        }
        matched.ok_or(ResumeError::Missing)
    }

    fn capture(book: BookId, books: &[Option<BookFile>]) -> Result<Self, ResumeError> {
        let file = books
            .get(book.index())
            .and_then(Option::as_ref)
            .ok_or(ResumeError::Missing)?;
        let identity = Self::from(file);
        identity.resolve(books)?;
        Ok(identity)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumeError {
    Missing,
    Ambiguous,
    LegacyInvalid,
    InvalidRecord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SavedPoint {
    Home(HomeItem),
    Books(Option<BookIdentity>),
    Files,
    FilesBook(BookIdentity),
    FilesImage(u32),
    Settings {
        selected: SettingsItem,
        draft: AppPreferences,
    },
    Reader {
        book: BookIdentity,
        spine_index: u32,
        page_index: u32,
        origin: BookOrigin,
    },
    Image(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SavedResume {
    point: SavedPoint,
    preferences: AppPreferences,
}

impl SavedResume {
    pub fn capture(
        resume: ResumePoint,
        preferences: AppPreferences,
        books: &[Option<BookFile>],
    ) -> Result<Self, ResumeError> {
        let point = match resume {
            ResumePoint::Home { selected } => SavedPoint::Home(selected),
            ResumePoint::Books { selected } => SavedPoint::Books(
                selected
                    .map(|book| BookIdentity::capture(book, books))
                    .transpose()?,
            ),
            ResumePoint::Files { selected: None } => SavedPoint::Files,
            ResumePoint::Files {
                selected: Some(file),
            } if file.index() < books.len() => {
                SavedPoint::FilesBook(BookIdentity::capture(BookId::new(file.index()), books)?)
            }
            ResumePoint::Files {
                selected: Some(file),
            } => SavedPoint::FilesImage(pack_index(file.index() - books.len())?),
            ResumePoint::Settings { selected, draft } => SavedPoint::Settings { selected, draft },
            ResumePoint::Reader {
                book,
                spine_index,
                page_index,
                origin,
            } => SavedPoint::Reader {
                book: BookIdentity::capture(book, books)?,
                spine_index: pack_index(spine_index)?,
                page_index: pack_index(page_index)?,
                origin,
            },
            ResumePoint::Image { image } => SavedPoint::Image(pack_index(image.index())?),
        };
        Ok(Self { point, preferences })
    }

    pub const fn preferences(&self) -> AppPreferences {
        self.preferences
    }

    pub fn resolve(&self, books: &[Option<BookFile>]) -> Result<ResumePoint, ResumeError> {
        Ok(match self.point {
            SavedPoint::Home(selected) => ResumePoint::Home { selected },
            SavedPoint::Books(book) => ResumePoint::Books {
                selected: book.map(|book| book.resolve(books)).transpose()?,
            },
            SavedPoint::Files => ResumePoint::Files { selected: None },
            SavedPoint::FilesBook(book) => ResumePoint::Files {
                selected: Some(FileId::new(book.resolve(books)?.index())),
            },
            SavedPoint::FilesImage(image) => ResumePoint::Files {
                selected: Some(FileId::new(
                    books
                        .len()
                        .checked_add(unpack_index(image)?)
                        .ok_or(ResumeError::InvalidRecord)?,
                )),
            },
            SavedPoint::Settings { selected, draft } => ResumePoint::Settings { selected, draft },
            SavedPoint::Reader {
                book,
                spine_index,
                page_index,
                origin,
            } => ResumePoint::Reader {
                book: book.resolve(books)?,
                spine_index: unpack_index(spine_index)?,
                page_index: unpack_index(page_index)?,
                origin,
            },
            SavedPoint::Image(image) => ResumePoint::Image {
                image: ImageId::new(unpack_index(image)?),
            },
        })
    }

    pub fn encode(&self) -> [u32; RESUME_WORDS] {
        let mut words = [0; RESUME_WORDS];
        words[0] = RESUME_MAGIC;
        words[6] = self.preferences.packed();
        let identity = match self.point {
            SavedPoint::Home(selected) => {
                words[1] = HOME_KIND;
                words[2] = selected.index() as u32;
                None
            }
            SavedPoint::Books(book) => {
                words[1] = BOOKS_KIND;
                words[2] = if book.is_some() { 0 } else { u32::MAX };
                book
            }
            SavedPoint::Files => {
                words[1] = FILES_KIND;
                None
            }
            SavedPoint::FilesBook(book) => {
                words[1] = FILES_BOOK_KIND;
                Some(book)
            }
            SavedPoint::FilesImage(image) => {
                words[1] = FILES_IMAGE_KIND;
                words[2] = image;
                None
            }
            SavedPoint::Settings { selected, draft } => {
                words[1] = SETTINGS_KIND;
                words[2] = selected.index() as u32;
                words[5] = draft.packed();
                None
            }
            SavedPoint::Reader {
                book,
                spine_index,
                page_index,
                origin,
            } => {
                words[1] = READER_KIND;
                words[3] = spine_index;
                words[4] = page_index;
                words[5] = origin.index() as u32;
                Some(book)
            }
            SavedPoint::Image(image) => {
                words[1] = IMAGE_KIND;
                words[2] = image;
                None
            }
        };
        if let Some(identity) = identity {
            let name = identity.name.as_str().as_bytes();
            words[8] = name.len() as u32;
            words[9] = identity.size;
            for (index, &byte) in name.iter().enumerate() {
                words[10 + index / 4] |= u32::from(byte) << (index % 4 * 8);
            }
        }
        words[CHECKSUM_WORD] = checksum(&words);
        words
    }

    pub fn decode(words: &[u32]) -> Result<Self, ResumeError> {
        if words.first() == Some(&LEGACY_MAGIC) {
            return Err(ResumeError::LegacyInvalid);
        }
        if words.len() != RESUME_WORDS
            || words[0] != RESUME_MAGIC
            || words[CHECKSUM_WORD] != checksum(words)
        {
            return Err(ResumeError::InvalidRecord);
        }
        let preferences =
            AppPreferences::from_packed(words[6]).ok_or(ResumeError::InvalidRecord)?;
        let point = match words[1] {
            HOME_KIND => SavedPoint::Home(
                HomeItem::from_index(unpack_index(words[2])?).ok_or(ResumeError::InvalidRecord)?,
            ),
            BOOKS_KIND if words[2] == u32::MAX => SavedPoint::Books(None),
            BOOKS_KIND => SavedPoint::Books(Some(decode_identity(words)?)),
            FILES_KIND => SavedPoint::Files,
            FILES_BOOK_KIND => SavedPoint::FilesBook(decode_identity(words)?),
            FILES_IMAGE_KIND => {
                unpack_index(words[2])?;
                SavedPoint::FilesImage(words[2])
            }
            SETTINGS_KIND => SavedPoint::Settings {
                selected: SettingsItem::from_index(unpack_index(words[2])?)
                    .ok_or(ResumeError::InvalidRecord)?,
                draft: AppPreferences::from_packed(words[5]).ok_or(ResumeError::InvalidRecord)?,
            },
            READER_KIND => {
                unpack_index(words[3])?;
                unpack_index(words[4])?;
                SavedPoint::Reader {
                    book: decode_identity(words)?,
                    spine_index: words[3],
                    page_index: words[4],
                    origin: BookOrigin::from_index(unpack_index(words[5])?)
                        .ok_or(ResumeError::InvalidRecord)?,
                }
            }
            IMAGE_KIND => {
                unpack_index(words[2])?;
                SavedPoint::Image(words[2])
            }
            _ => return Err(ResumeError::InvalidRecord),
        };
        let saved = Self { point, preferences };
        if saved.encode() != words {
            return Err(ResumeError::InvalidRecord);
        }
        Ok(saved)
    }
}

fn decode_identity(words: &[u32]) -> Result<BookIdentity, ResumeError> {
    let length = unpack_index(words[8])?;
    let mut bytes = [0; MAX_BOOK_NAME_BYTES];
    for (chunk, word) in bytes.chunks_mut(4).zip(&words[10..]) {
        chunk.copy_from_slice(&word.to_le_bytes()[..chunk.len()]);
    }
    let name = core::str::from_utf8(bytes.get(..length).ok_or(ResumeError::InvalidRecord)?)
        .map_err(|_| ResumeError::InvalidRecord)?;
    Ok(BookIdentity {
        name: BookFileName::try_from(name).map_err(|_| ResumeError::InvalidRecord)?,
        size: words[9],
    })
}

fn pack_index(index: usize) -> Result<u32, ResumeError> {
    u32::try_from(index)
        .ok()
        .filter(|&index| index != u32::MAX)
        .ok_or(ResumeError::InvalidRecord)
}

fn unpack_index(index: u32) -> Result<usize, ResumeError> {
    if index == u32::MAX {
        return Err(ResumeError::InvalidRecord);
    }
    usize::try_from(index).map_err(|_| ResumeError::InvalidRecord)
}

fn checksum(words: &[u32]) -> u32 {
    words
        .iter()
        .enumerate()
        .filter(|&(index, _)| index != CHECKSUM_WORD)
        .fold(0x811C_9DC5, |hash, (_, &word)| {
            (hash ^ word).wrapping_mul(0x0100_0193)
        })
}

#[cfg(test)]
mod tests;
