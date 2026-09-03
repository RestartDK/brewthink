#![no_std]

#[cfg(feature = "epub")]
extern crate std;

pub mod app;
#[cfg(feature = "device-reader")]
pub mod bounded_layout;
#[cfg(feature = "device-reader")]
pub mod bounded_xml;
#[cfg(feature = "device-reader")]
pub mod cover;
#[cfg(feature = "device-reader")]
pub mod device_epub;
pub mod diagnostics;
pub mod display;
#[cfg(feature = "epub")]
pub mod epub;
pub mod files;
mod fonts;
pub mod home;
pub mod image;
pub mod input;
pub mod library;
pub mod power;
pub mod reader;
pub mod settings;
pub mod sleep;
pub mod storage;
pub mod ui;
pub mod x4;
#[cfg(feature = "device-reader")]
pub mod zip_stream;
