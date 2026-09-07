use crate::bounded_xml::{FixedString, XmlError, XmlEvent, XmlReader};

pub const CHAPTER_TITLE_BYTES: usize = 64;

pub fn read_entries(encoded: &[u8], mut entry: impl FnMut(&str, &str)) -> Result<(), XmlError> {
    let mut reader = XmlReader::new(encoded)?;
    let mut depth = 0usize;
    let mut scope = None;
    let mut ncx = false;
    let mut anchor = None;
    let mut text = None;
    let mut href = FixedString::<256>::new();
    let mut title = FixedString::<CHAPTER_TITLE_BYTES>::new();
    let mut truncated = false;
    while let Some(event) = reader.next_event()? {
        match event {
            XmlEvent::Start(tag) => {
                let level = depth + 1;
                if level > 128 {
                    return Err(XmlError::Malformed);
                }
                let name = tag.local_name();
                if scope.is_none()
                    && (name == "navMap"
                        || (name == "nav"
                            && tag
                                .attribute("type")?
                                .unwrap_or("")
                                .split_ascii_whitespace()
                                .any(|value| value == "toc")))
                {
                    scope = Some(level);
                    ncx = name == "navMap";
                }
                if scope.is_some() {
                    if !ncx && name == "a" {
                        href = FixedString::try_from_str(tag.attribute("href")?.unwrap_or(""))?;
                        title.clear();
                        truncated = false;
                        anchor = Some(level);
                        text = Some(level);
                    } else if ncx && name == "navPoint" {
                        title.clear();
                        truncated = false;
                    } else if ncx && name == "text" {
                        text = Some(level);
                    } else if ncx && name == "content" {
                        let src =
                            FixedString::<256>::try_from_str(tag.attribute("src")?.unwrap_or(""))?;
                        title.normalize_whitespace();
                        if !src.is_empty() && !title.is_empty() {
                            entry(src.as_str(), title.as_str());
                        }
                    }
                }
                depth = level;
            }
            XmlEvent::Text(value) if text.is_some() => {
                for character in value {
                    let character = character?;
                    if !truncated && title.push(character).is_err() {
                        truncated = true;
                    }
                }
            }
            XmlEvent::End(name) => {
                if depth == 0 {
                    return Err(XmlError::Malformed);
                }
                if anchor == Some(depth) {
                    if name != "a" {
                        return Err(XmlError::Malformed);
                    }
                    title.normalize_whitespace();
                    if !href.is_empty() && !title.is_empty() {
                        entry(href.as_str(), title.as_str());
                    }
                    anchor = None;
                }
                if text == Some(depth) {
                    text = None;
                }
                if scope == Some(depth) {
                    scope = None;
                }
                depth -= 1;
            }
            XmlEvent::Text(_) => {}
        }
    }
    if depth != 0 {
        return Err(XmlError::Malformed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{string::ToString, vec::Vec};

    #[test]
    fn epub3_reads_only_the_toc_and_preserves_inline_text() {
        let mut entries = Vec::new();
        read_entries(br#"<html><nav epub:type="landmarks"><a href="bad">Ignore</a></nav><nav epub:type="toc"><ol><li><a href="one.xhtml#start">One &amp; <span>two</span></a><ol><li><a href="two.xhtml">Nested</a></li></ol></li></ol></nav></html>"#, |href, title| entries.push((href.to_string(), title.to_string()))).unwrap();
        assert_eq!(
            entries,
            [
                ("one.xhtml#start".to_string(), "One & two".to_string()),
                ("two.xhtml".to_string(), "Nested".to_string())
            ]
        );
    }

    #[test]
    fn ncx_keeps_parent_and_nested_labels_separate() {
        let mut entries = Vec::new();
        read_entries(br#"<ncx><navMap><navPoint><navLabel><text>Parent</text></navLabel><content src="p.xhtml"/><navPoint><navLabel><text>Child</text></navLabel><content src="c.xhtml#id"/></navPoint></navPoint></navMap></ncx>"#, |href, title| entries.push((href.to_string(), title.to_string()))).unwrap();
        assert_eq!(entries[0], ("p.xhtml".to_string(), "Parent".to_string()));
        assert_eq!(entries[1], ("c.xhtml#id".to_string(), "Child".to_string()));
    }

    #[test]
    fn bounds_utf8_titles_and_rejects_truncated_xml() {
        let source = std::format!(
            "<nav type='toc'><a href='one'>{}</a></nav>",
            "é".repeat(100)
        );
        read_entries(source.as_bytes(), |_, title| {
            assert_eq!(title, "é".repeat(32))
        })
        .unwrap();
        assert!(read_entries(b"<nav type='toc'><a href='one'>Title", |_, _| {}).is_err());
    }
}
