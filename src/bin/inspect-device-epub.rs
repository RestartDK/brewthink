use std::{env, error::Error, fs};

use brewthink::{
    bounded_layout::layout_xhtml_page,
    cover::{COVER_BYTES, CoverDecodeWorkspace, decode_png_cover},
    device_epub::{DeviceEpub, DevicePackageScratch, MAX_DEVICE_RESOURCE_BYTES},
    zip_stream::{InflateWorkspace, ReadAt, ZipValidationScratch},
};

struct SliceFile<'a>(&'a [u8]);

impl ReadAt for SliceFile<'_> {
    type Error = std::convert::Infallible;

    fn len(&self) -> u32 {
        self.0.len() as u32
    }

    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        let start = offset as usize;
        let count = self.0.len().saturating_sub(start).min(output.len());
        output[..count].copy_from_slice(&self.0[start..start + count]);
        Ok(count)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = env::args()
        .nth(1)
        .ok_or("usage: inspect-device-epub <book.epub>")?;
    let encoded = fs::read(path)?;
    let mut zip_scratch = Box::new(ZipValidationScratch::new());
    let mut package_scratch = Box::new(DevicePackageScratch::new());
    let mut inflater = Box::new(InflateWorkspace::new());
    let mut resource = Box::new([0; MAX_DEVICE_RESOURCE_BYTES]);
    let book = DeviceEpub::open(
        SliceFile(&encoded),
        &mut zip_scratch,
        &mut package_scratch,
        &mut inflater,
        &mut resource,
    )
    .map_err(|error| format!("{error:?}"))?;

    println!("title: {}", book.publication().title());
    println!("creator: {}", book.publication().creator());
    println!("spine: {}", book.publication().spine_len());
    println!(
        "cover: {}",
        book.publication().cover_path().unwrap_or("none")
    );
    if let Some(length) = book
        .read_cover(&mut resource[..], &mut inflater)
        .map_err(|error| format!("{error:?}"))?
    {
        let mut packed = Box::new([0; COVER_BYTES]);
        let mut decoder = Box::new(CoverDecodeWorkspace::new());
        decode_png_cover(&resource[..length], &mut packed, &mut decoder)
            .map_err(|error| format!("cover decode: {error:?}"))?;
        let fingerprint = packed.iter().fold(0x811C_9DC5u32, |hash, byte| {
            (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
        });
        println!("cover-bytes: {length}, packed-fingerprint: {fingerprint:08x}");
    }
    for index in 0..book.publication().spine_len() {
        let length = book
            .read_spine(index, &mut resource[..], &mut inflater)
            .map_err(|error| format!("{error:?}"))?;
        let first = layout_xhtml_page(&resource[..length], 0)
            .map_err(|error| format!("spine {index} layout: {error:?}"))?;
        let last = layout_xhtml_page(&resource[..length], first.page_count() - 1)
            .map_err(|error| format!("spine {index} last page: {error:?}"))?;
        println!(
            "spine-{index}: {length} bytes, {} pages, {}",
            first.page_count(),
            first.chapter_title()
        );
        assert_eq!(last.page_index() + 1, first.page_count());
    }
    Ok(())
}
