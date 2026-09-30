# Per-book reading position

The reader keeps one record per book on the card, in `/brew/bookmark`, so
switching books no longer discards where the other one was left. Records are
`XXXXXXXX.BMK` keyed by the CRC of the book's filename and size, with
`XXXXXXXX.BAK` holding the previous record and `XXXXXXXX.TMP` used during
publication.

Reading a record requires an exact filename and size match and a valid
checksum, so a slot collision or a half-written file cannot hand back another
book's position. Publication writes the temporary record, verifies it, copies
the current record to the backup, replaces the record, verifies that, and
removes the temporary file. Every interrupted boundary leaves either the old
or the new record readable, which the fault-injection tests assert boundary by
boundary. That is not a claim of crash-consistent FAT metadata.

`App` holds the open book's position in memory and asks the adapter for the
stored record only when it opens a book it has no in-memory position for. The
adapter saves after a page is on the panel and again before deep sleep, and it
skips the write when the stored record already matches. A failed read or write
is reported on the control channel, not swallowed.

The record holds the chapter, the page, the page count, and the typography that
produced it. Reopening with the same typography uses the exact page; reopening
after a typography change maps the old progress onto the new page count. The
position is independent of the disposable chapter and image caches, so
dropping those caches does not move the reader.
