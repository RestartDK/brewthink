extern crate std;

use super::*;
use std::{format, string::String, vec::Vec};

fn lines(xml: &str) -> Vec<String> {
    layout_xhtml_page(xml.as_bytes(), 0, ReaderPreferences::default())
        .unwrap()
        .lines()
        .map(|line| String::from(line.text()))
        .collect()
}

#[test]
fn cdata_preserves_ampersands_and_entity_spelling() {
    assert_eq!(lines("<body><p><![CDATA[A & B]]></p></body>")[0], "A & B");
    assert_eq!(
        lines("<body><p><![CDATA[A &amp; B]]></p></body>")[0],
        "A &amp; B"
    );
    assert_eq!(lines("<body><p>A &amp;amp; B</p></body>")[0], "A &amp; B");
}

#[test]
fn preformatted_lines_preserve_indentation_and_blank_lines() {
    let output = lines("<body><pre>first\n    second\n\n  third</pre></body>");
    assert_eq!(&output[..4], &["first", "    second", "", "  third"]);
}

#[test]
fn preformatted_crlf_and_tabs_have_stable_columns() {
    let output = lines("<body><pre>a\r\n\tb</pre></body>");
    assert_eq!(&output[..2], &["a", "    b"]);
}

#[test]
fn rejects_mismatched_unclosed_and_multiple_roots() {
    for xml in [
        "<body><p>Hello</other></body>",
        "<body><p>Hello</p>",
        "<body></body><body></body>",
        "<body><a:p xmlns:a='a' xmlns:b='b'>Hello</b:p></body>",
    ] {
        assert!(
            layout_xhtml_page(xml.as_bytes(), 0, ReaderPreferences::default()).is_err(),
            "{xml}"
        );
    }
}

#[test]
fn bounds_xml_nesting() {
    let xml = format!(
        "<body>{}text{}</body>",
        "<span>".repeat(256),
        "</span>".repeat(256)
    );
    assert!(layout_xhtml_page(xml.as_bytes(), 0, ReaderPreferences::default()).is_err());
}

#[test]
fn wrapping_keeps_ordinary_words_whole_across_inline_tags() {
    for count in 5..80 {
        let xml = format!(
            "<body><p>{}deve<em>lop</em>ment</p></body>",
            "a ".repeat(count)
        );
        let output = lines(&xml);
        assert!(
            output.iter().any(|line| line.contains("development")),
            "{output:?}"
        );
    }
}

#[test]
fn oversized_words_split_without_loss_or_overwide_lines() {
    for word in ["development".repeat(50), "é\u{301}🙂".repeat(150)] {
        let xml = format!("<body><p>{word}</p></body>");
        let preferences = ReaderPreferences::default();
        let first = layout_xhtml_page(xml.as_bytes(), 0, preferences).unwrap();
        let mut output = Vec::new();
        for index in 0..first.page_count() {
            let page = layout_xhtml_page(xml.as_bytes(), index, preferences).unwrap();
            assert_eq!(page.page_count(), first.page_count());
            output.extend(page.lines().map(|line| String::from(line.text())));
        }
        let theme = ReaderTheme::from_preferences(ReaderPreferences::default());
        for line in &output {
            assert!(
                theme.text_width(ReaderStyle::Body, line) <= theme.line_width(ReaderStyle::Body)
            );
        }
        assert_eq!(output.concat(), word);
    }
}

#[test]
fn empty_blocks_do_not_leak_preformatted_style() {
    assert_eq!(lines("<body><pre/>a  b</body>")[0], "a b");
    assert_eq!(lines("<body><p>A&#160;B</p></body>")[0], "A\u{a0}B");
    let page = layout_xhtml_page(
        b"<body><blockquote/>text</body>",
        0,
        ReaderPreferences::default(),
    )
    .unwrap();
    assert_eq!(page.lines().next().unwrap().style(), ReaderStyle::Body);
}
