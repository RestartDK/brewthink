#[cfg(feature = "sd-card")]
mod catalog;
pub mod diagnostic;
mod layout;
mod sdcard;
#[cfg(feature = "sd-write-diagnostic")]
mod write_test;

#[cfg(feature = "sd-card")]
pub use catalog::{
    APP_DATA_DIRECTORY, BOOK_DIRECTORY, BOOKMARK_DIRECTORY, BookCatalog, BookFile, BookFileName,
    BookFileNameError, CACHE_DIRECTORY, FILE_DIRECTORY, FatStorage,
};
#[cfg(feature = "device-reader")]
pub use catalog::{
    AppDataError, AppDataStore, FatFileReader, ImageCatalog, ImageFile, MAX_DEVICE_IMAGE_BYTES,
    StoredImage,
};
pub use layout::{
    DiskLayout, Filesystem, Partition, inspect_filesystem, inspect_sector_zero, sector_fingerprint,
};
#[cfg(feature = "sd-card-write")]
pub use sdcard::WritableSdCard;
pub use sdcard::{
    CardInfo, CardType, CardVersion, ReadOnlySdCard, ReadOnlySdSpi, SdError, SdProtocolError,
    SdSpiClock, Sector,
};
#[cfg(feature = "sd-write-diagnostic")]
pub use write_test::{
    TemporaryFilePhase, TemporaryFileStore, TemporaryFileTestError, create_verify_delete,
};
