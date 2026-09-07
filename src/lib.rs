#![no_std]

#[cfg(any(feature = "epub", feature = "web-sim"))]
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
#[cfg(feature = "image-decoder")]
pub mod image_decoder;
pub mod image_viewer;
pub mod input;
pub mod library;
pub mod power;
pub mod reader;
pub mod settings;
#[cfg(feature = "web-sim")]
pub mod simulator;
pub mod sleep;
pub mod storage;
#[cfg(feature = "image-decoder")]
pub mod transfer;
pub mod ui;
#[cfg(test)]
mod ui_contract;
pub mod x4;
#[cfg(feature = "device-reader")]
pub mod zip_stream;
