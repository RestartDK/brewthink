use super::{
    EpubBook,
    tests::{CONTAINER, PACKAGE, epub},
};
use std::{string::ToString, vec::Vec};

fn with_navigation(package: &str, nav: &[u8]) -> Vec<u8> {
    epub(&[
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OPS/book.opf", package.as_bytes()),
        (
            "OPS/text/chapter.xhtml",
            b"<html><body><p>Text</p></body></html>",
        ),
        ("OPS/text/nav.xhtml", nav),
    ])
}

#[test]
fn resolves_epub3_and_ncx_names_to_spine_items_not_fragments() {
    let ncx_package = PACKAGE
        .replace(
            "application/xhtml+xml\" properties=\"nav",
            "application/x-dtbncx+xml",
        )
        .replace("<spine>", "<spine toc=\"nav\">");
    for (package, nav) in [
        (PACKAGE, br#"<html><nav epub:type="landmarks"><a href="chapter.xhtml">Ignore</a></nav><nav epub:type="toc"><a href="%63hapter.xhtml#start">A named &amp; nested chapter</a><a href="chapter.xhtml#later">Later section</a><a href="missing.xhtml">Missing</a></nav></html>"#.as_slice()),
        (ncx_package.as_str(), br#"<ncx><navMap><navPoint><navLabel><text>A named &amp; nested chapter</text></navLabel><content src="%63hapter.xhtml#start"/></navPoint></navMap></ncx>"#.as_slice()),
    ] {
        let encoded = with_navigation(package, nav);
        let mut book = EpubBook::open(&encoded).unwrap();
        assert_eq!(book.chapter_titles(), [Some("A named & nested chapter".to_string())]);
        #[cfg(feature = "device-reader")]
        assert_device_title(&encoded, "A named & nested chapter");
    }
}

#[test]
fn missing_and_malformed_navigation_leave_chapter_fallbacks_available() {
    for nav in [
        b"<nav type='toc'><a href='chapter.xhtml'>Partial</a>".as_slice(),
        b"<html/>".as_slice(),
    ] {
        let encoded = with_navigation(PACKAGE, nav);
        assert_eq!(EpubBook::open(&encoded).unwrap().chapter_titles(), [None]);
        #[cfg(feature = "device-reader")]
        assert_device_title(&encoded, "");
    }
    let encoded = super::tests::epub(&[
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OPS/book.opf", PACKAGE.as_bytes()),
    ]);
    assert_eq!(EpubBook::open(&encoded).unwrap().chapter_titles(), [None]);
}

#[cfg(feature = "device-reader")]
fn assert_device_title(encoded: &[u8], expected: &str) {
    use crate::{bounded_xml::FixedString, device_epub::*, zip_stream::*};
    use std::boxed::Box;
    struct Bytes<'a>(&'a [u8]);
    impl ReadAt for Bytes<'_> {
        type Error = core::convert::Infallible;
        fn len(&self) -> u32 {
            self.0.len() as u32
        }
        fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
            let tail = self.0.get(offset as usize..).unwrap_or(&[]);
            let length = tail.len().min(output.len());
            output[..length].copy_from_slice(&tail[..length]);
            Ok(length)
        }
    }
    let mut zip = Box::new(ZipValidationScratch::new());
    let mut package = Box::new(DevicePackageScratch::new());
    let mut inflater = Box::new(InflateWorkspace::new());
    let mut buffer = Box::new([0; MAX_DEVICE_RESOURCE_BYTES]);
    let mut publication = Box::new(DevicePublication::new());
    let book = DeviceEpub::open(
        Bytes(encoded),
        &mut zip,
        &mut package,
        &mut inflater,
        &mut buffer,
        &mut publication,
    )
    .unwrap();
    let mut titles = [FixedString::new(); MAX_DEVICE_SPINE_ITEMS];
    let result = book.read_chapter_titles(&mut titles, buffer.as_mut(), &mut inflater);
    assert!(result.is_ok() || expected.is_empty());
    assert_eq!(titles[0].as_str(), expected);
}
