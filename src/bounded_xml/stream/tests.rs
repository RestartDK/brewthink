extern crate std;

use super::*;
use crate::bounded_xml::XmlReader;
use core::cell::Cell;
use std::{boxed::Box, format, string::String, vec::Vec};

#[derive(Debug, Eq, PartialEq)]
struct ReadFailure;

struct Input<'a> {
    bytes: &'a [u8],
    chunk: usize,
    fail_at: Option<u32>,
    reads: Cell<usize>,
}

impl<'a> Input<'a> {
    fn new(bytes: &'a [u8], chunk: usize) -> Self {
        Self {
            bytes,
            chunk,
            fail_at: None,
            reads: Cell::new(0),
        }
    }
}

impl ReadAt for Input<'_> {
    type Error = ReadFailure;
    fn len(&self) -> u32 {
        self.bytes.len() as u32
    }
    fn read_at(&self, offset: u32, output: &mut [u8]) -> Result<usize, Self::Error> {
        assert!(output.len() <= INPUT_BYTES);
        if self.fail_at.is_some_and(|at| offset >= at) {
            return Err(ReadFailure);
        }
        self.reads.set(self.reads.get() + 1);
        let bytes = &self.bytes[offset as usize..];
        let count = output.len().min(bytes.len()).min(self.chunk);
        output[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Event {
    Start(String, Option<String>),
    End(String),
    Text(String),
}

fn collect(event: XmlEvent<'_>, output: &mut Vec<Event>) {
    match event {
        XmlEvent::Start(tag) => output.push(Event::Start(
            tag.local_name().into(),
            tag.attribute("href").unwrap().map(Into::into),
        )),
        XmlEvent::End(name) => output.push(Event::End(name.into())),
        XmlEvent::Text(text) => {
            let text: String = text.map(Result::unwrap).collect();
            if text.is_empty() {
                return;
            }
            if let Some(Event::Text(previous)) = output.last_mut() {
                previous.push_str(&text);
            } else {
                output.push(Event::Text(text));
            }
        }
    }
}

fn stream(input: &Input<'_>) -> Result<Vec<Event>, StreamXmlError<ReadFailure>> {
    let mut workspace = Box::new(XmlWorkspace::new());
    let mut reader = XmlStream::new(input, &mut workspace);
    let mut output = Vec::new();
    while let Some(event) = reader.next_event()? {
        collect(event, &mut output);
    }
    Ok(output)
}

#[test]
fn every_small_read_boundary_preserves_tags_utf8_entities_and_cdata() {
    let xml = format!(
        "\u{feff}<?xml version='1.0'?><!DOCTYPE html [<!ENTITY ignored 'a>b'>]><html xmlns:x='urn:test'><body><x:p href='a>b'>å文🦀&amp;&#x1f980;&#32;{}<![CDATA[{}]]></x:p><br/><p>tail</p></body></html><!--tail-->",
        "word&amp;".repeat(1300),
        "]x & >\u{1f980}".repeat(1000)
    );
    let mut original = XmlReader::new(xml.as_bytes()).unwrap();
    let mut expected = Vec::new();
    while let Some(event) = original.next_event().unwrap() {
        collect(event, &mut expected);
    }
    for chunk in [1, 2, 3, 4, 5, 7, 13, 31, 127, 511, 1024] {
        assert_eq!(
            stream(&Input::new(xml.as_bytes(), chunk)).unwrap(),
            expected,
            "chunk {chunk}"
        );
    }
}

#[test]
fn large_text_comments_and_processing_instructions_do_not_grow_workspace() {
    assert!(core::mem::size_of::<XmlWorkspace>() <= 14 * 1024);
    let words = "unchanged text å文. ".repeat(100_000);
    let xml = format!(
        "<html><!--{}--><?ignored {}?><body><p>{words}</p></body></html>",
        "-x".repeat(100_000),
        "?x".repeat(100_000)
    );
    let input = Input::new(xml.as_bytes(), INPUT_BYTES);
    let events = stream(&input).unwrap();
    assert!(events.contains(&Event::Text(words)));
    assert_eq!(input.reads.get(), xml.len().div_ceil(INPUT_BYTES));
}

#[test]
fn malformed_tails_and_split_invalid_text_cannot_publish_a_complete_document() {
    let cases: &[(&[u8], XmlError)] = &[
        (b"<html><body>valid</body>", XmlError::Malformed),
        (b"<a><b></a></b>", XmlError::Malformed),
        (b"<a/>&#32;", XmlError::Malformed),
        (b"<a>&missing;</a>", XmlError::InvalidEntity),
        (b"<a>&amp</a>", XmlError::InvalidEntity),
        (b"<a><![CDATA[unfinished</a>", XmlError::Malformed),
        (b"<a/> trailing", XmlError::Malformed),
        (b"<a/><b/>", XmlError::Malformed),
        (b"<a><!--\xff--></a>", XmlError::InvalidUtf8),
        (b"<a>\xf0\x9f\xa6</a>", XmlError::InvalidUtf8),
        (b"<a>\x00</a>", XmlError::Malformed),
    ];
    for &(bytes, error) in cases {
        assert_eq!(
            stream(&Input::new(bytes, 1)),
            Err(StreamXmlError::Xml(error)),
            "{bytes:?}"
        );
    }
    let deep = format!(
        "{}{}",
        "<p>".repeat(MAX_XML_DEPTH + 1),
        "</p>".repeat(MAX_XML_DEPTH + 1)
    );
    assert_eq!(
        stream(&Input::new(deep.as_bytes(), 7)),
        Err(StreamXmlError::Xml(XmlError::NestingTooDeep))
    );
    let attribute = format!("<a href='{}'/>", "x".repeat(TOKEN_BYTES));
    assert_eq!(
        stream(&Input::new(attribute.as_bytes(), 19)),
        Err(StreamXmlError::Xml(XmlError::OutputFull))
    );
}

#[test]
fn short_reads_continue_but_read_failures_and_false_eof_fail() {
    let xml = b"<html><body>the complete chapter</body></html>";
    let mut input = Input::new(xml, 2);
    assert!(stream(&input).is_ok());
    input.fail_at = Some(12);
    assert_eq!(stream(&input), Err(StreamXmlError::Read(ReadFailure)));
    assert_eq!(
        stream(&Input::new(xml, 0)),
        Err(StreamXmlError::Xml(XmlError::Malformed))
    );
}
