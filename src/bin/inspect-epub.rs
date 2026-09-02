use std::{env, fs, path::PathBuf, process::ExitCode};

use brewthink::epub::EpubBook;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let path = parse_path()?;
    let encoded =
        fs::read(&path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mut book = EpubBook::open(&encoded).map_err(|error| format!("invalid EPUB: {error:?}"))?;
    let publication = book.publication();

    println!("path: {}", path.display());
    println!("epub-version: {}", publication.version());
    println!("title: {}", publication.metadata().title());
    println!(
        "creator: {}",
        publication
            .metadata()
            .primary_creator()
            .unwrap_or("Unknown")
    );
    println!(
        "language: {}",
        publication.metadata().language().unwrap_or("Unknown")
    );
    println!("resources: {}", publication.resources().len());
    println!("spine-items: {}", publication.spine().len());

    let cover = publication
        .cover()
        .map(|resource| (resource.path().to_owned(), resource.media_type().to_owned()));
    match cover {
        Some((path, media_type)) => {
            let bytes = book
                .read_cover()
                .map_err(|error| format!("cover read failed: {error:?}"))?
                .ok_or_else(|| "manifest cover is missing from the ZIP archive".to_owned())?;
            println!("cover-path: {path}");
            println!("cover-media-type: {media_type}");
            println!("cover-bytes: {}", bytes.len());
        }
        None => println!("cover-path: none"),
    }
    Ok(())
}

fn parse_path() -> Result<PathBuf, String> {
    let mut arguments = env::args_os();
    let program = arguments.next().unwrap_or_default();
    let path = arguments
        .next()
        .ok_or_else(|| format!("usage: {} <book.epub>", PathBuf::from(program).display()))?;
    if arguments.next().is_some() {
        return Err("expected exactly one EPUB path".to_owned());
    }
    Ok(path.into())
}
