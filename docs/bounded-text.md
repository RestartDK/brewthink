# Bounded reader text

The device reader consumes XML events and stores only the requested page. It does not build a chapter DOM.

`XmlText` distinguishes encoded XML text from literal CDATA. Its iterator decodes character references once for encoded text and leaves CDATA spelling intact. `FixedString` can consume that iterator through `TryFrom`. The package parser uses the same path for title and creator text.

`XmlReader` requires one complete root element, matches full qualified start and end names, rejects text outside the root except XML whitespace, and limits open elements to `MAX_XML_DEPTH`, currently 64. Empty elements do not consume stack depth. They emit a start/end pair so layout and metadata consumers share one element lifecycle. The container parser reads to the end before returning the selected package path. The parser does not expand DTD entities or validate XML schemas.

Normal layout buffers one word across adjacent text and inline-tag events. It wraps at whitespace and splits only words that exceed a line's pixel width or bounded byte capacity. Numeric non-breaking spaces remain part of the word. Both the current line and pending word are capped at 320 UTF-8 bytes.

Preformatted blocks retain newlines, blank lines, indentation, and trailing spaces. CRLF produces one line break. Tabs advance to four-column stops. Long preformatted lines still wrap at the reader's pixel and byte limits.

These fixes change device pagination. Persisted page numbers can refer to different text after a layout change. Semantic reading positions are separate work. The browser still uses its existing reading pipeline until the simulator parity PR.

`FixedString` rejects capacities above `u16::MAX` at compile time, so its length cannot wrap. XML and layout errors implement `Display` and `core::error::Error`, and layout preserves the XML cause.

## Verification

The regression tests cover literal CDATA, single entity decoding, XML structure and depth boundaries, preformatted whitespace, ordinary and oversized words, and full-text reconstruction across pages. Host package tests cover CDATA metadata and malformed content after a container rootfile entry. No physical device operation is required.
