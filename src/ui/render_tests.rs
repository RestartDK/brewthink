extern crate std;

use std::{fs, path::PathBuf};

use ::image::{GrayImage, Luma};

use crate::{
    app::{
        App, AppEffect, AppInput, AppPreferences, AppView, BookMeta, FilesState, LibraryIndex,
        LibraryState, ReaderPreferences, SeriesMeta, SeriesPosition, SettingsItem, SettingsState,
        SleepScreenMode,
    },
    files::{FileItem, FileKind},
    image::{PackedBitmap, PackedImage, READER_DEPTH, Size},
    input::UsbState,
    power::BatteryStatus,
    reader::{ReaderLine, ReaderStyle, ReaderView},
    settings::CustomImagePreview,
    sleep::SleepView,
    ui::{AppFrame, render_app},
};

fn library_index(books: &[BookMeta<'_>]) -> LibraryIndex {
    LibraryIndex::try_from(books).unwrap()
}

const WIDTH: usize = 480;
const HEIGHT: usize = 800;
const MONO_FRAME_BYTES: usize = WIDTH * HEIGHT / 8;
const FRAME_BYTES: usize = MONO_FRAME_BYTES * READER_DEPTH.bits();

#[test]
fn application_frames_match_snapshots() {
    let battery = BatteryStatus::from_percent(82, UsbState::Disconnected);

    assert_frame("home", |target| {
        render_app(
            AppFrame::Home {
                state: App::new(crate::app::test_index(4)).home(),
                battery,
            },
            target,
        )
        .unwrap();
    });

    let books = [
        BookMeta::new(
            "The Art of Doing Science and Engineering: Learning to Learn",
            "Richard W. Hamming",
            None,
        ),
        BookMeta::new(
            "A Study in Scarlet",
            "Arthur Conan Doyle",
            Some(SeriesMeta::new(
                "Sherlock Holmes",
                Some(SeriesPosition::new(1)),
            )),
        ),
        BookMeta::new("Pride and Prejudice", "Jane Austen", None),
        BookMeta::new(
            "The Sign of the Four",
            "Arthur Conan Doyle",
            Some(SeriesMeta::new(
                "Sherlock Holmes",
                Some(SeriesPosition::new(2)),
            )),
        ),
    ];
    let index = library_index(&books);
    assert_frame("library", |target| {
        render_app(
            AppFrame::Library {
                view: LibraryState::new(index).view(&books).unwrap(),
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
fn storage_and_sleep_frames_match_snapshots() {
    let battery = BatteryStatus::from_percent(82, UsbState::Disconnected);
    assert_frame("library-empty", |target| {
        let books: [BookMeta; 0] = [];
        let index = library_index(&books);
        render_app(
            AppFrame::Library {
                view: LibraryState::new(index).view(&books).unwrap(),
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
                        target.set_luma(x, y, if (x + y) % 2 == 0 { 0 } else { 255 });
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
    let cover = PackedBitmap::monochrome(Size::new(176, 264).unwrap(), &cover_bytes).unwrap();
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
        let bytes = std::vec![0xaa; FRAME_BYTES];
        let bitmap =
            PackedBitmap::new(Size::new(WIDTH, HEIGHT).unwrap(), READER_DEPTH, &bytes).unwrap();
        render_app(AppFrame::Sleep(SleepView::book_cover(bitmap)), target).unwrap();
    });
    assert_frame("sleep-custom", |target| {
        let bytes = std::vec![0xaa; FRAME_BYTES];
        let bitmap =
            PackedBitmap::new(Size::new(WIDTH, HEIGHT).unwrap(), READER_DEPTH, &bytes).unwrap();
        render_app(AppFrame::Sleep(SleepView::custom(bitmap)), target).unwrap();
    });
}

#[test]
fn library_pages_and_series_match_snapshots() {
    let battery = BatteryStatus::from_percent(82, UsbState::Disconnected);
    let books = [
        BookMeta::new("Book One", "An Author", None),
        BookMeta::new("Book Two", "An Author", None),
        BookMeta::new("Book Three", "An Author", None),
        BookMeta::new("Book Four", "An Author", None),
        BookMeta::new("Book Five", "An Author", None),
        BookMeta::new("Book Six", "An Author", None),
        BookMeta::new("Book Seven", "An Author", None),
        BookMeta::new("Book Eight", "An Author", None),
        BookMeta::new("Book Nine", "An Author", None),
        BookMeta::new("Book Ten", "An Author", None),
    ];
    let index = library_index(&books);
    for page in [0, 1] {
        let mut state = LibraryState::new(index);
        for _ in 0..page * 8 {
            state.move_selection(crate::app::Direction::Down);
        }
        assert_frame(&std::format!("library-page-{page}"), |target| {
            render_app(
                AppFrame::Library {
                    view: state.view(&books).unwrap(),
                    battery,
                },
                target,
            )
            .unwrap();
        });
    }

    let series = [
        BookMeta::new(
            "Volume Three",
            "An Author",
            Some(SeriesMeta::new(
                "Collected Works",
                Some(SeriesPosition::new(3)),
            )),
        ),
        BookMeta::new("A Standalone Novel", "Another Author", None),
        BookMeta::new(
            "Volume One",
            "An Author",
            Some(SeriesMeta::new(
                "Collected Works",
                Some(SeriesPosition::new(1)),
            )),
        ),
        BookMeta::new(
            "Volume Two",
            "An Author",
            Some(SeriesMeta::new(
                "Collected Works",
                Some(SeriesPosition::new(2)),
            )),
        ),
    ];
    let index = library_index(&series);
    let mut state = LibraryState::new(index);
    assert!(matches!(
        state.activate(),
        crate::app::LibraryActivation::EnteredSeries
    ));
    assert_frame("library-series", |target| {
        render_app(
            AppFrame::Library {
                view: state.view(&series).unwrap(),
                battery,
            },
            target,
        )
        .unwrap();
    });
}

#[test]
fn reader_drawer_matches_snapshot() {
    let mut app = App::new(crate::app::test_index(1));
    assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
    assert_eq!(
        app.input_without_stored_progress(AppInput::Confirm),
        AppEffect::Render
    );
    assert!(matches!(
        app.input(AppInput::Confirm),
        AppEffect::LoadChapter { .. }
    ));
    assert_eq!(app.chapter_loaded(2, 3).unwrap(), AppEffect::Render);
    app.input(AppInput::Confirm);
    let lines = [ReaderLine::new("The page stays behind.", ReaderStyle::Body)];
    let AppView::ReaderDrawer(drawer) = app.view() else {
        panic!("drawer expected")
    };
    assert_frame("reader-drawer-0", |target| {
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
}

fn assert_frame(name: &str, render: impl FnOnce(&mut PackedImage<'_>)) {
    let size = Size::new(WIDTH, HEIGHT).unwrap();
    let mut bytes = std::vec![0xFF; FRAME_BYTES];
    let mut image = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
    render(&mut image);

    let actual = GrayImage::from_fn(WIDTH as u32, HEIGHT as u32, |x, y| {
        Luma([image.luma(x as usize, y as usize)])
    });
    let path = fixture_path(name);
    if std::env::var_os("BLESS_UI_FRAMES").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        actual.save(&path).unwrap();
    }
    let expected = ::image::open(&path)
        .unwrap_or_else(|error| panic!("could not read snapshot {}: {error}", path.display()))
        .into_luma8();
    if expected != actual {
        let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("artifacts/render-diffs")
            .join(std::format!("{name}.png"));
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        actual.save(&output).unwrap();
        panic!("frame {name} changed; inspect {}", output.display());
    }
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ui")
        .join(std::format!("{name}.png"))
}
