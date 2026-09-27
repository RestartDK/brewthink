use std::{
    convert::Infallible,
    fs, io,
    path::{Path, PathBuf},
};

use brewthink::{
    app::ReaderPreferences,
    bounded_layout::layout_xhtml_page,
    device_epub::{DeviceEpub, DevicePackageScratch, DevicePublication, MAX_DEVICE_RESOURCE_BYTES},
    epub::EpubBook,
    transfer::{BookName, MAX_BOOK_BYTES},
    zip_stream::{InflateWorkspace, ReadAt, ZipValidationScratch},
};

pub struct PreparedBook {
    pub path: PathBuf,
    pub name: BookName,
    pub bytes: Vec<u8>,
    title: String,
    warnings: Vec<String>,
}

impl PreparedBook {
    pub fn read(path: &Path) -> io::Result<Self> {
        if fs::metadata(path)?.len() > MAX_BOOK_BYTES as u64 {
            return Err(super::invalid_input("book exceeds the 32 MiB upload limit"));
        }
        let bytes = fs::read(path)?;
        if bytes.len() > MAX_BOOK_BYTES {
            return Err(super::invalid_input("book exceeds the 32 MiB upload limit"));
        }
        let publication = EpubBook::open(&bytes)
            .map_err(|error| super::invalid_input(format!("{}: {error:?}", path.display())))?;
        let title = publication.publication().metadata().title().to_owned();
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let name = BookName::parse(&format!("{stem}.EPB")).unwrap_or_else(|_| {
            BookName::parse(&format!("{:08X}.EPB", crc32fast::hash(&bytes)))
                .expect("eight hexadecimal digits form a valid book filename")
        });
        let warnings = reader_warnings(&bytes);
        Ok(Self {
            path: path.to_owned(),
            name,
            bytes,
            title,
            warnings,
        })
    }

    pub fn report(&self) {
        println!(
            "host: book {:?} -> /books/{} bytes={}",
            self.title,
            self.name.as_str(),
            self.bytes.len()
        );
        for warning in &self.warnings {
            println!("host: reader warning for {}: {warning}", self.name.as_str());
        }
        if self.warnings.is_empty() {
            println!(
                "host: {} passed device package and chapter layout checks; cover not checked",
                self.name.as_str()
            );
        }
    }
}

pub fn prepare_directory(directory: &Path) -> io::Result<Vec<PreparedBook>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry.path().extension().is_some_and(|extension| {
                extension.eq_ignore_ascii_case("epub") || extension.eq_ignore_ascii_case("epb")
            })
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(super::invalid_input("directory contains no EPUB files"));
    }
    let mut books: Vec<PreparedBook> = Vec::new();
    for path in paths {
        let book = PreparedBook::read(&path)?;
        if let Some(existing) = books.iter().find(|existing| existing.name == book.name) {
            if existing.bytes != book.bytes {
                return Err(super::invalid_input(format!(
                    "short filename collision: {}",
                    book.name.as_str()
                )));
            }
            continue;
        }
        books.push(book);
    }
    Ok(books)
}

struct Bytes<'a>(&'a [u8]);

impl ReadAt for Bytes<'_> {
    type Error = Infallible;

    fn len(&self) -> u32 {
        self.0.len() as u32
    }

    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        let start = (offset as usize).min(self.0.len());
        let count = (self.0.len() - start).min(output.len());
        output[..count].copy_from_slice(&self.0[start..start + count]);
        Ok(count)
    }
}

fn reader_warnings(encoded: &[u8]) -> Vec<String> {
    let mut zip = Box::new(ZipValidationScratch::new());
    let mut package = Box::new(DevicePackageScratch::new());
    let mut inflater = Box::new(InflateWorkspace::new());
    let mut resource = Box::new([0; MAX_DEVICE_RESOURCE_BYTES]);
    let mut publication = Box::new(DevicePublication::new());
    let book = match DeviceEpub::open(
        Bytes(encoded),
        &mut zip,
        &mut package,
        &mut inflater,
        &mut resource,
        &mut publication,
    ) {
        Ok(book) => book,
        Err(error) => {
            return vec![format!(
                "not currently readable or listed by device: {error:?}"
            )];
        }
    };
    let mut warnings = Vec::new();
    for index in 0..book.publication().spine_len() {
        match book.read_spine(index, &mut resource[..], &mut inflater) {
            Ok(length) => {
                if let Err(error) =
                    layout_xhtml_page(&resource[..length], 0, ReaderPreferences::default())
                {
                    warnings.push(format!("chapter {} layout: {error:?}", index + 1));
                }
            }
            Err(error) => warnings.push(format!("chapter {} read: {error:?}", index + 1)),
        }
    }
    warnings
}
