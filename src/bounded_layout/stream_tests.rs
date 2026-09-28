extern crate std;

use super::*;
use crate::{bounded_xml::stream::XmlWorkspace, image::Size, zip_stream::ReadAt};
use std::{boxed::Box, format, vec::Vec};

struct Source<'a>(&'a [u8], usize);
impl ReadAt for Source<'_> {
    type Error = &'static str;
    fn len(&self) -> u32 {
        self.0.len() as u32
    }
    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        let bytes = &self.0[offset as usize..];
        let count = bytes.len().min(output.len()).min(self.1);
        output[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }
}

fn image(path: &str) -> Option<ImageResource> {
    Some(ImageResource {
        path: FixedString::try_from_str(path).ok()?,
        size: Size::new(320, 240).unwrap(),
        crc32: 17,
        bytes: 230_000,
    })
}

#[test]
fn streamed_pages_match_slice_layout_with_text_images_and_late_title() {
    let text = format!(
        "<html><body><p>{}</p><h1>Late title</h1><blockquote><p>Quote &amp; text</p></blockquote><pre>  α\r\n\tβ</pre><img src='figure.png' alt='Figure'/><p>{}</p><table><tr><td>A</td><td>B</td></tr></table></body></html>",
        "å文 ordinary words <em>across</em> tags. ".repeat(1000),
        "longunbrokenword".repeat(400)
    );
    for chunk in [1, 13, 1024] {
        let mut workspace = Box::new(XmlWorkspace::new());
        let mut page = Box::new(BoundedPage::new());
        let mut pages = Vec::new();
        let summary = layout_xhtml_stream(
            &Source(text.as_bytes(), chunk),
            &mut workspace,
            ReaderPreferences::default(),
            &mut page,
            |href| Ok(image(href)),
            |page| {
                pages.push(*page);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(pages.len(), summary.page_count);
        for (index, page) in pages.iter_mut().enumerate() {
            page.page_count = summary.page_count;
            page.chapter_title = summary.title;
            let mut expected = Box::new(BoundedPage::new());
            layout_xhtml_page_with_images_into(
                text.as_bytes(),
                index,
                ReaderPreferences::default(),
                &mut expected,
                image,
            )
            .unwrap();
            assert_eq!(page, expected.as_mut(), "chunk={chunk} page={index}");
        }
    }
}

#[test]
fn multi_megabyte_paragraph_emits_pages_before_eof_without_losing_text() {
    let phrase = "One α word. ";
    let repeats = 150_000;
    let text = format!(
        "<html><body><p>{}</p></body></html>",
        phrase.repeat(repeats)
    );
    assert!(text.len() > 1_000_000);
    let mut workspace = Box::new(XmlWorkspace::new());
    let mut page = Box::new(BoundedPage::new());
    let mut words = 0;
    let mut pages = 0;
    let summary = layout_xhtml_stream(
        &Source(text.as_bytes(), 1024),
        &mut workspace,
        ReaderPreferences::default(),
        &mut page,
        |_| Ok(None),
        |page| {
            assert_eq!(page.page_index(), pages);
            pages += 1;
            words += page
                .lines()
                .map(|line| line.text().split_whitespace().count())
                .sum::<usize>();
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(words, 3 * repeats);
    assert_eq!(pages, summary.page_count);
    assert!(pages > 1000);
}

#[test]
fn malformed_tail_and_writer_failure_never_return_a_complete_summary() {
    let text = format!("<html><body><p>{}</p></body>", "word ".repeat(2000));
    let mut workspace = Box::new(XmlWorkspace::new());
    let mut page = Box::new(BoundedPage::new());
    let mut written = 0;
    let result = layout_xhtml_stream(
        &Source(text.as_bytes(), 1024),
        &mut workspace,
        ReaderPreferences::default(),
        &mut page,
        |_| Ok(None),
        |_| {
            written += 1;
            Ok(())
        },
    );
    assert!(written > 0);
    assert_eq!(
        result,
        Err(StreamLayoutError::Layout(LayoutError::Xml(
            XmlError::Malformed
        )))
    );
    let result = layout_xhtml_stream(
        &Source(text.as_bytes(), 1024),
        &mut workspace,
        ReaderPreferences::default(),
        &mut page,
        |_| Ok(None),
        |_| Err("card full"),
    );
    assert_eq!(result, Err(StreamLayoutError::Write("card full")));
}
