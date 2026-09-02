#[cfg(feature = "sd-card")]
mod catalog;
mod layout;
mod sdcard;
#[cfg(feature = "sd-write-diagnostic")]
mod write_test;

#[cfg(feature = "device-reader")]
pub use catalog::FatBookReader;
#[cfg(feature = "sd-card")]
pub use catalog::{
    BOOK_DIRECTORY, BookCatalog, BookFile, BookFileName, BookFileNameError, ReadOnlyFatBookStore,
};
pub use layout::{
    DiskLayout, Filesystem, Partition, inspect_filesystem, inspect_sector_zero, sector_fingerprint,
};
#[cfg(feature = "sd-write-diagnostic")]
pub use sdcard::ExplicitWriteSdCard;
pub use sdcard::{
    CardInfo, CardType, CardVersion, ReadOnlySdCard, ReadOnlySdSpi, SdError, SdProtocolError,
    SdSpiClock, Sector,
};
#[cfg(feature = "sd-write-diagnostic")]
pub use write_test::{
    TemporaryFilePhase, TemporaryFileStore, TemporaryFileTestError, create_verify_delete,
};
