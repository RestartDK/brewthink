use crate::{
    app::{FilesState, HomeState, LibraryState, SettingsState},
    files::{FileItem, FilesRenderError, render_files},
    home::{HomeRenderError, render_home},
    image::{PackedBitmap, PackedImage},
    library::{ShelfBook, ShelfRenderError, render_shelf},
    power::BatteryStatus,
    reader::{ReaderRenderError, ReaderView, render_reader, render_reader_error},
    settings::{CustomImagePreview, SettingsRenderError, render_settings},
    sleep::{SleepRenderError, SleepView, render_sleep},
};

#[derive(Clone, Copy)]
pub enum AppFrame<'a> {
    Home {
        state: HomeState,
        battery: BatteryStatus,
    },
    Library {
        state: LibraryState,
        books: &'a [ShelfBook<'a>],
        battery: BatteryStatus,
    },
    Files {
        state: FilesState,
        files: &'a [FileItem<'a>],
        battery: BatteryStatus,
    },
    Settings {
        state: SettingsState,
        battery: BatteryStatus,
        custom_image: CustomImagePreview<'a>,
    },
    Cover(PackedBitmap<'a>),
    Reader(ReaderView<'a>),
    Sleep(SleepView<'a>),
    Error {
        book_title: &'a str,
        message: &'a str,
        battery: BatteryStatus,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppRenderError {
    Home(HomeRenderError),
    Library(ShelfRenderError),
    Files(FilesRenderError),
    Settings(SettingsRenderError),
    Cover(SleepRenderError),
    Reader(ReaderRenderError),
    Sleep(SleepRenderError),
}

pub fn render_app(frame: AppFrame<'_>, target: &mut PackedImage<'_>) -> Result<(), AppRenderError> {
    match frame {
        AppFrame::Home { state, battery } => {
            render_home(state, battery, target).map_err(AppRenderError::Home)
        }
        AppFrame::Library {
            state,
            books,
            battery,
        } => render_shelf(state, books, battery, target).map_err(AppRenderError::Library),
        AppFrame::Files {
            state,
            files,
            battery,
        } => render_files(state, files, battery, target).map_err(AppRenderError::Files),
        AppFrame::Settings {
            state,
            battery,
            custom_image,
        } => {
            render_settings(state, battery, custom_image, target).map_err(AppRenderError::Settings)
        }
        AppFrame::Cover(bitmap) => {
            render_sleep(SleepView::book_cover(bitmap), target).map_err(AppRenderError::Cover)
        }
        AppFrame::Reader(view) => render_reader(view, target).map_err(AppRenderError::Reader),
        AppFrame::Sleep(view) => render_sleep(view, target).map_err(AppRenderError::Sleep),
        AppFrame::Error {
            book_title,
            message,
            battery,
        } => render_reader_error(book_title, message, battery, target)
            .map_err(AppRenderError::Reader),
    }
}
