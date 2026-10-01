# Per-book reading position

The reader keeps one record per book on the card, in `/brew/bookmark`, so
switching books no longer discards where the other one was left. Records are
`XXXXXXXX.BMK` keyed by the CRC of the book's filename and size, with
`XXXXXXXX.BAK` holding the previous record and `XXXXXXXX.TMP` used during
publication. Collisions probe up to 32 consecutive slots, wrapping at the
32-bit limit. Reads and writes use the same sequence and match the full
identity before reusing a slot. Exhaustion reports an error without replacing
another book's record.

Reading a record requires an exact filename and size match and a valid
checksum, so a slot collision or a half-written file cannot hand back another
book's position. Publication writes the temporary record, verifies it, copies
a valid current record for the same book to the backup, and replaces the primary.
It then verifies the primary and removes the temporary file. A corrupt primary
leaves the previous backup intact. Every interrupted boundary leaves either
the old or the new record readable, which the fault-injection tests assert
boundary by boundary. That is not a claim of crash-consistent FAT metadata.

`App` holds the open book's position in memory and asks the adapter for the
stored record only when it opens a book it has no in-memory position for. The
adapter saves after a page is on the panel and again before deep sleep using
the checkpoint, even when the current view is `Sleeping`. It skips the write
when the stored record already matches. Storage read and write failures are
reported on the control channel. A failed progress load opens the cover
instead of preventing reading. Invalid records report a typed decode cause
unless a valid backup recovers the position.

The record holds the chapter, the page, the page count, and the typography that
produced it. Reopening with the same typography uses the exact page; reopening
after a typography change maps the old progress onto the new page count. A
restored chapter that cannot load discards the in-memory checkpoint and falls
back to the cover. Reading from there replaces the invalid stored position. The
position is independent of the disposable chapter and image caches, so
dropping those caches does not move the reader.

# Resume after the processor powers off

The reader also mirrors the encoded resume record to `/brew/LAST.BIN` before
deep sleep, with `LAST.BAK` holding the previous record and `LAST.TMP` used
during publication. On battery the board's power-path latch powers the
processor off completely during deep sleep, RTC memory included, so the retained
record cannot survive and the next Power press is a cold boot. The boot path
reads the card record when RTC memory holds no valid record and restores the
same book, chapter, page, and origin. It skips the write when the stored record
already matches, and the same interrupted-write rules as the bookmarks apply:
every boundary leaves the old or the new record readable.

On USB power the processor stays powered during deep sleep and the RTC record
is used directly. The card record only fills in when RTC memory is empty or
invalid, so a reset or a battery power-off returns to the open book instead of
Home.
