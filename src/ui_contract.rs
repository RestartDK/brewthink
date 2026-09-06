extern crate std;

use std::{fs, path::PathBuf};

use crate::{
    app::{
        App, AppEffect, AppInput, AppPreferences, AppView, FilesState, LibraryState,
        ReaderPreferences, SettingsItem, SettingsState, SleepScreenMode,
    },
    files::{FileItem, FileKind},
    image::{MonochromeBitmap, MonochromeImage, Size},
    input::UsbState,
    library::ShelfBook,
    power::BatteryStatus,
    reader::{ReaderLine, ReaderStyle, ReaderView},
    sleep::{CustomSleepImageStatus, SleepView},
    ui::{AppFrame, render_app},
};

const WIDTH: usize = 480;
const HEIGHT: usize = 800;
const FRAME_BYTES: usize = WIDTH * HEIGHT / 8;
const PBM_HEADER: &[u8] = b"P4\n480 800\n";

#[test]
fn application_frames_match_the_pinned_contract() {
    let battery = BatteryStatus::from_percent(82, UsbState::Disconnected);

    assert_frame("home", |target| {
        render_app(
            AppFrame::Home {
                state: App::new(4).home(),
                battery,
            },
            target,
        )
        .unwrap();
    });

    let books = [
        ShelfBook::new("The First Book", "Author One", None),
        ShelfBook::new("The Second Book", "Author Two", None),
        ShelfBook::new("The Third Book", "Author Three", None),
        ShelfBook::new("The Fourth Book", "Author Four", None),
    ];
    assert_frame("library", |target| {
        render_app(
            AppFrame::Library {
                state: LibraryState::new(books.len()),
                books: &books,
                battery,
            },
            target,
        )
        .unwrap();
    });

    let files = [
        FileItem::new("first.epub", 12_345, FileKind::Epub),
        FileItem::new("second.jpg", 67_890, FileKind::Jpeg),
        FileItem::new("third.png", 1_024, FileKind::Png),
    ];
    assert_frame("files", |target| {
        render_app(
            AppFrame::Files {
                state: FilesState::new(files.len()),
                files: &files,
                battery,
            },
            target,
        )
        .unwrap();
    });

    assert_frame("settings", |target| {
        render_app(
            AppFrame::Settings {
                state: SettingsState::new(AppPreferences::default()),
                battery,
                custom_image_status: CustomSleepImageStatus::Missing,
                custom_image_name: None,
                custom_image_preview: None,
            },
            target,
        )
        .unwrap();
    });

    let location = reader_location();
    let lines = [
        ReaderLine::new("A Declarative Reader", ReaderStyle::Heading),
        ReaderLine::new("The body follows the heading.", ReaderStyle::Body),
        ReaderLine::new("A quoted line.", ReaderStyle::Quote),
    ];
    assert_frame("reader", |target| {
        render_app(
            AppFrame::Reader(ReaderView::new(
                "The First Book",
                "Chapter One",
                &lines,
                location,
                ReaderPreferences::default(),
                battery,
            )),
            target,
        )
        .unwrap();
    });

    assert_frame("error", |target| {
        render_app(
            AppFrame::Error {
                book_title: "The First Book",
                message: "This EPUB or chapter could not be opened.",
                battery,
            },
            target,
        )
        .unwrap();
    });

    assert_frame("sleep", |target| {
        render_app(
            AppFrame::Sleep(SleepView::built_in("HOME POSITION SAVED", battery)),
            target,
        )
        .unwrap();
    });
}

#[test]
fn storage_and_sleep_frames_match_the_pinned_contract() {
    let battery = BatteryStatus::from_percent(82, UsbState::Disconnected);
    assert_frame("library-empty", |target| {
        render_app(
            AppFrame::Library {
                state: LibraryState::new(0),
                books: &[],
                battery,
            },
            target,
        )
        .unwrap();
    });
    for selected in [false, true] {
        assert_frame(
            if selected { "image-selected" } else { "image" },
            |target| {
                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        target.set_pixel(x, y, (x + y) % 2 == 0);
                    }
                }
                crate::image_viewer::render_image_viewer("cover.jpg", selected, battery, target)
                    .unwrap();
            },
        );
    }
    assert_frame("files-empty", |target| {
        render_app(
            AppFrame::Files {
                state: FilesState::new(0),
                files: &[],
                battery,
            },
            target,
        )
        .unwrap();
    });
    assert_frame("files-long-name", |target| {
        let files = [FileItem::new(
            "a-long-image-name-that-must-not-overwrite-the-file-size.png",
            12_345,
            FileKind::Png,
        )];
        render_app(
            AppFrame::Files {
                state: FilesState::new(1),
                files: &files,
                battery,
            },
            target,
        )
        .unwrap();
    });
    let cover_bytes = [0xAA; 176 * 264 / 8];
    let cover = MonochromeBitmap::new(Size::new(176, 264).unwrap(), &cover_bytes).unwrap();
    for (name, mode, status, preview) in [
        (
            "settings-sleep-auto",
            SleepScreenMode::Automatic,
            CustomSleepImageStatus::Missing,
            None,
        ),
        (
            "settings-sleep-custom",
            SleepScreenMode::Custom,
            CustomSleepImageStatus::Ready,
            Some(cover),
        ),
        (
            "settings-sleep-invalid",
            SleepScreenMode::Custom,
            CustomSleepImageStatus::Invalid,
            None,
        ),
        (
            "settings-sleep-cover",
            SleepScreenMode::BookCover,
            CustomSleepImageStatus::Missing,
            None,
        ),
    ] {
        assert_frame(name, |target| {
            render_app(
                AppFrame::Settings {
                    state: SettingsState::with_state(
                        SettingsItem::SleepScreen,
                        AppPreferences::new(ReaderPreferences::default(), mode),
                    ),
                    battery,
                    custom_image_status: status,
                    custom_image_name: Some("cover.jpg"),
                    custom_image_preview: preview,
                },
                target,
            )
            .unwrap();
        });
    }
    assert_frame("sleep-cover", |target| {
        render_app(
            AppFrame::Sleep(SleepView::book_cover(
                "The First Book",
                "Author One",
                "POSITION SAVED",
                cover,
                battery,
            )),
            target,
        )
        .unwrap();
    });
    assert_frame("sleep-custom", |target| {
        let bytes = std::vec![0xAA; FRAME_BYTES];
        let bitmap = MonochromeBitmap::new(Size::new(WIDTH, HEIGHT).unwrap(), &bytes).unwrap();
        render_app(AppFrame::Sleep(SleepView::custom(bitmap, battery)), target).unwrap();
    });
}

fn reader_location() -> crate::app::ReadingLocation {
    let mut app = App::new(1);
    assert_eq!(app.input(AppInput::Confirm), AppEffect::RenderLibrary);
    assert!(matches!(
        app.input(AppInput::Confirm),
        AppEffect::LoadChapter { .. }
    ));
    let effect = app.chapter_loaded(2, 3).unwrap();
    let AppEffect::RenderReader(location) = effect else {
        panic!("chapter load did not produce a reader frame");
    };
    assert!(matches!(app.view(), AppView::Reader(_)));
    location
}

fn assert_frame(name: &str, render: impl FnOnce(&mut MonochromeImage<'_>)) {
    let mut bytes = std::vec![0xFF; FRAME_BYTES];
    let mut image = MonochromeImage::new(Size::new(WIDTH, HEIGHT).unwrap(), &mut bytes).unwrap();
    render(&mut image);

    let mut encoded = std::vec::Vec::with_capacity(PBM_HEADER.len() + FRAME_BYTES);
    encoded.extend_from_slice(PBM_HEADER);
    encoded.extend(bytes.iter().map(|byte| !byte));

    let path = fixture_path(name);
    if std::env::var_os("BLESS_UI_FRAMES").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &encoded).unwrap();
    }

    let expected = fs::read(&path)
        .unwrap_or_else(|error| panic!("could not read pinned frame {}: {error}", path.display()));
    if expected == encoded {
        return;
    }

    let difference = expected
        .iter()
        .zip(&encoded)
        .position(|(expected, actual)| expected != actual)
        .unwrap_or(expected.len().min(encoded.len()));
    panic!(
        "frame {name} changed at byte {difference}: expected {} bytes, rendered {} bytes",
        expected.len(),
        encoded.len()
    );
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ui")
        .join(std::format!("{name}.pbm"))
}
