use crate::app::{
    App, AppEffect, AppInput, AppPreferences, AppStateError, BookId, ImageId, ReaderPreferences,
    ResumePoint,
};
use crate::{
    storage::{ImageCatalog, ImageFile},
    transfer::ImageName,
};

pub struct ReaderImages<const CAPACITY: usize> {
    pub(crate) files: [Option<ImageFile>; CAPACITY],
    pub(crate) length: usize,
}

impl<const CAPACITY: usize> ReaderImages<CAPACITY> {
    pub const fn empty() -> Self {
        Self {
            files: [None; CAPACITY],
            length: 0,
        }
    }

    pub fn file(&self, image: ImageId) -> Option<&ImageFile> {
        self.files.get(image.index()).and_then(Option::as_ref)
    }

    pub fn scanned<E>(&mut self, result: Result<ImageCatalog<CAPACITY>, E>) -> Result<(), E> {
        let catalog = result?;
        *self = Self::empty();
        for image in catalog.images() {
            self.files[self.length] = Some(image);
            self.length += 1;
        }
        Ok(())
    }

    pub fn selected<E>(&self, record: Result<Option<ImageName>, E>) -> Result<Option<ImageId>, E> {
        Ok(record?
            .and_then(|name| {
                self.files[..self.length]
                    .iter()
                    .flatten()
                    .position(|image| *image.name() == name)
                    .map(ImageId::new)
            })
            .or_else(|| (self.length > 0).then(|| ImageId::new(0))))
    }

    pub fn apply_selection<E>(
        &self,
        app: &mut App,
        selection: Result<Option<ImageId>, E>,
    ) -> Result<(), E> {
        match selection {
            Ok(selected) => {
                app.replace_image_catalog(self.length, selected);
                Ok(())
            }
            Err(error) => {
                app.replace_image_catalog(self.length, None);
                app.sleep_image_unavailable();
                Err(error)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupStage {
    Card,
    Layout,
    Books,
    Images,
    Display,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Startup {
    Run(StartupStage),
    AwaitingRetry(StartupStage),
    Ready,
}

impl StartupStage {
    fn next(self) -> Startup {
        match self {
            Self::Card => Startup::Run(Self::Layout),
            Self::Layout => Startup::Run(Self::Books),
            Self::Books => Startup::Run(Self::Images),
            Self::Images => Startup::Run(Self::Display),
            Self::Display => Startup::Ready,
        }
    }
}

impl Startup {
    pub fn complete<E>(&mut self, result: Result<(), E>) -> Result<(), E> {
        let Self::Run(stage) = *self else {
            return result;
        };
        match result {
            Ok(()) => {
                *self = stage.next();
                Ok(())
            }
            Err(error) => {
                *self = Self::AwaitingRetry(stage);
                Err(error)
            }
        }
    }

    pub fn input(&mut self, input: AppInput) {
        if let Self::AwaitingRetry(stage) = *self {
            match input {
                AppInput::Confirm => *self = Self::Run(stage),
                AppInput::Back
                    if matches!(
                        stage,
                        StartupStage::Layout | StartupStage::Books | StartupStage::Images
                    ) =>
                {
                    *self = stage.next()
                }
                _ => {}
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChapterPages {
    pub spine_count: usize,
    pub page_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rendered {
    Frame,
    CoverAbsent,
}

pub fn render_cover<E>(
    decoded: Result<bool, E>,
    unavailable: impl FnOnce(&E),
    refresh: impl FnOnce() -> Result<(), E>,
) -> Result<Rendered, E> {
    let available = decoded.unwrap_or_else(|error| {
        unavailable(&error);
        false
    });
    if !available {
        return Ok(Rendered::CoverAbsent);
    }
    refresh()?;
    Ok(Rendered::Frame)
}

pub trait ReaderIo {
    type Error;

    fn load_chapter(
        &mut self,
        book: BookId,
        spine_index: usize,
        preferences: ReaderPreferences,
    ) -> Result<ChapterPages, Self::Error>;

    fn render(&mut self, app: &App) -> Result<Rendered, Self::Error>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum Failure<E> {
    Chapter(E),
    Render(E),
    State(AppStateError),
    EffectLimit,
}

#[derive(Debug, Eq, PartialEq)]
pub struct OperationFailure<E> {
    pub cause: Failure<E>,
    pub recovery: Option<Failure<E>>,
}

pub fn persist_input_preferences<T, E>(
    result: Result<T, E>,
    previous: AppPreferences,
    app: &App,
    persist: impl FnOnce(AppPreferences),
) -> Result<T, E> {
    if app.preferences() != previous {
        persist(app.preferences());
    }
    result
}

pub fn run_effect<I: ReaderIo>(
    effect: AppEffect,
    app: &mut App,
    io: &mut I,
) -> Result<Option<ResumePoint>, OperationFailure<I::Error>> {
    drive_effect(effect, app, io).map_err(|cause| {
        let effect = app.recover_operation();
        let recovery = drive_effect(effect, app, io).err();
        if recovery.is_some() {
            app.recover_operation();
        }
        OperationFailure { cause, recovery }
    })
}

fn drive_effect<I: ReaderIo>(
    mut effect: AppEffect,
    app: &mut App,
    io: &mut I,
) -> Result<Option<ResumePoint>, Failure<I::Error>> {
    const MAX_EFFECT_STEPS: usize = 4;
    for _ in 0..MAX_EFFECT_STEPS {
        effect = match effect {
            AppEffect::None => return Ok(None),
            AppEffect::EnterDeepSleep { resume } => return Ok(Some(resume)),
            AppEffect::LoadChapter {
                book, spine_index, ..
            } => {
                let chapter = io
                    .load_chapter(book, spine_index, app.reader_preferences())
                    .map_err(Failure::Chapter)?;
                app.chapter_loaded(chapter.spine_count, chapter.page_count)
                    .map_err(Failure::State)?
            }
            AppEffect::Render => match io.render(app).map_err(Failure::Render)? {
                Rendered::CoverAbsent => app.input(AppInput::Confirm),
                Rendered::Frame => {
                    if matches!(app.view(), crate::app::AppView::Sleeping { .. }) {
                        app.sleep_frame_ready().map_err(Failure::State)?
                    } else {
                        return Ok(None);
                    }
                }
            },
        };
    }
    Err(Failure::EffectLimit)
}

#[cfg(test)]
mod tests;
