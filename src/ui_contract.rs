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
    settings::CustomImagePreview,
    sleep::SleepView,
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
                custom_image: CustomImagePreview::Missing,
            },
            target,
        )
        .unwrap();
    });

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
            AppFrame::Sleep(SleepView::built_in("Position saved", battery)),
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
    for (name, mode, custom_image) in [
        (
            "settings-sleep-auto",
            SleepScreenMode::Automatic,
            CustomImagePreview::Missing,
        ),
        (
            "settings-sleep-custom",
            SleepScreenMode::Custom,
            CustomImagePreview::Ready {
                name: "cover.jpg",
                bitmap: cover,
            },
        ),
        (
            "settings-sleep-invalid",
            SleepScreenMode::Custom,
            CustomImagePreview::Invalid,
        ),
        (
            "settings-sleep-cover",
            SleepScreenMode::BookCover,
            CustomImagePreview::Missing,
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
                    custom_image,
                },
                target,
            )
            .unwrap();
        });
    }
    assert_frame("sleep-cover", |target| {
        render_app(AppFrame::Sleep(SleepView::book_cover(cover)), target).unwrap();
    });
    assert_frame("sleep-custom", |target| {
        let bytes = std::vec![0xAA; FRAME_BYTES];
        let bitmap = MonochromeBitmap::new(Size::new(WIDTH, HEIGHT).unwrap(), &bytes).unwrap();
        render_app(AppFrame::Sleep(SleepView::custom(bitmap)), target).unwrap();
    });
}

#[test]
fn populated_shelf_and_settings_rows_match_the_pinned_contract() {
    let battery = BatteryStatus::from_percent(82, UsbState::Disconnected);
    let half_bytes = [0xAA; 88 * 132 / 8];
    let full_bytes = [0x33; 176 * 264 / 8];
    let half = MonochromeBitmap::new(Size::new(88, 132).unwrap(), &half_bytes).unwrap();
    let full = MonochromeBitmap::new(Size::new(176, 264).unwrap(), &full_bytes).unwrap();
    let covers = [Some(half), None, Some(half), Some(full), Some(full)];
    let books = covers.map(|cover| ShelfBook::new("A Book", "An Author", cover));
    for selected in [3, 4] {
        let state = LibraryState::with_selected(books.len(), selected).unwrap();
        assert_frame(&std::format!("library-page-{}", state.page()), |target| {
            render_app(
                AppFrame::Library {
                    state,
                    books: &books,
                    battery,
                },
                target,
            )
            .unwrap();
        });
    }
    for item in SettingsItem::ALL {
        assert_frame(&std::format!("settings-row-{}", item.index()), |target| {
            render_app(
                AppFrame::Settings {
                    state: SettingsState::with_state(item, AppPreferences::default()),
                    battery,
                    custom_image: CustomImagePreview::Missing,
                },
                target,
            )
            .unwrap();
        });
    }
}

#[test]
fn reader_drawer_rows_match_the_pinned_contract() {
    let mut app = App::new(1);
    assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
    assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
    assert!(matches!(
        app.input(AppInput::Confirm),
        AppEffect::LoadChapter { .. }
    ));
    assert_eq!(app.chapter_loaded(2, 3).unwrap(), AppEffect::Render);
    app.input(AppInput::Confirm);
    let lines = [ReaderLine::new("The page stays behind.", ReaderStyle::Body)];
    for row in 0..crate::app::ReaderControl::ALL.len() {
        let AppView::ReaderDrawer(drawer) = app.view() else {
            panic!("drawer expected")
        };
        assert_frame(&std::format!("reader-drawer-{row}"), |target| {
            render_app(
                AppFrame::Reader(
                    ReaderView::new(
                        "The First Book",
                        "Chapter one",
                        &lines,
                        app.reader_preferences(),
                        app.battery(),
                    )
                    .with_drawer(drawer),
                ),
                target,
            )
            .unwrap();
        });
        app.input(AppInput::Move(crate::app::Direction::Down));
    }
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
