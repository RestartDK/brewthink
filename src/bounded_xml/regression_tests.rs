extern crate std;

use super::*;
use std::{format, string::String, vec::Vec};

fn parse(xml: &[u8]) -> Result<Vec<XmlEvent<'_>>, XmlError> {
    let mut reader = XmlReader::new(xml)?;
    let mut events = Vec::new();
    while let Some(event) = reader.next_event()? {
        events.push(event);
    }
    Ok(events)
}

#[test]
fn encoded_and_literal_text_have_distinct_iterators() {
    assert_eq!(
        XmlText::Encoded("A &amp;amp; B")
            .collect::<Result<String, _>>()
            .unwrap(),
        "A &amp; B"
    );
    assert_eq!(
        XmlText::Literal("A &amp;amp; B")
            .collect::<Result<String, _>>()
            .unwrap(),
        "A &amp;amp; B"
    );
    assert_eq!(
        XmlText::Encoded("&#65;&#x1F600;")
            .collect::<Result<String, _>>()
            .unwrap(),
        "A😀"
    );
}

#[test]
fn invalid_entities_and_xml_characters_fail() {
    for text in [
        "&",
        "&unknown;",
        "&#0;",
        "&#xD800;",
        "&#xFFFF;",
        "&#+65;",
        "&#x+41;",
        "&#X41;",
    ] {
        assert_eq!(
            XmlText::Encoded(text).next(),
            Some(Err(XmlError::InvalidEntity)),
            "{text}"
        );
    }
    assert_eq!(
        XmlReader::new(b"<r>\0</r>").err(),
        Some(XmlError::Malformed)
    );
    assert_eq!(
        XmlReader::new(b"<r>\xff</r>").err(),
        Some(XmlError::InvalidUtf8)
    );
}

#[test]
fn empty_elements_do_not_consume_nesting_capacity() {
    let xml = format!("<root>{}</root>", "<child/>".repeat(256));
    assert_eq!(parse(xml.as_bytes()).unwrap().len(), 2 * (256 + 1));
}

#[test]
fn nesting_limit_accepts_the_boundary_and_rejects_one_more() {
    for depth in [MAX_XML_DEPTH, MAX_XML_DEPTH + 1] {
        let xml = format!("{}text{}", "<r>".repeat(depth), "</r>".repeat(depth));
        let result = parse(xml.as_bytes());
        if depth == MAX_XML_DEPTH {
            assert!(result.is_ok());
        } else {
            assert_eq!(result, Err(XmlError::NestingTooDeep));
        }
    }
}

#[test]
fn only_whitespace_comments_and_instructions_surround_the_root() {
    assert!(parse(b"\xef\xbb\xbf<?xml version='1.0'?> \n<root/> <!-- end -->\n").is_ok());
    for xml in [
        "",
        "text<r/>",
        "<r/>text",
        "<![CDATA[text]]><r/>",
        "<r><!DOCTYPE r></r>",
    ] {
        assert!(parse(xml.as_bytes()).is_err(), "{xml}");
    }
}

#[test]
fn fixed_string_preserves_utf8_at_its_capacity() {
    let mut small = FixedString::<4>::try_from(XmlText::Literal("éé")).unwrap();
    assert_eq!(small.push('a'), Err(XmlError::OutputFull));
    assert_eq!(small.as_str(), "éé");
    let mut largest = FixedString::<{ u16::MAX as usize }>::new();
    for _ in 0..u16::MAX {
        largest.push('a').unwrap();
    }
    assert_eq!(largest.push('a'), Err(XmlError::OutputFull));
    assert_eq!(largest.as_str().len(), u16::MAX as usize);
}
