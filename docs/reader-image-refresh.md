# Returning from grayscale to text

On the installed reader, leaving a grayscale page selected a monochrome
`full-clean` refresh lasting 4,171 ms. Returning from another image to the
library selected the same mode and took 4,172 ms. These are individual driver
measurements, not optical recordings or end-to-end page-turn times.

This document records the change and its measured effect. The device was
running the modified firmware for every timing below except the 4,170 ms
before value.

A successful grayscale update leaves the controller asleep. The next
monochrome update must reset and initialize it: its previous-frame RAM is not
a usable differential baseline. The driver previously combined that known
state with failed grayscale updates and forced full cleaning for both.

The candidate transition keeps the reset, absolute update, and baseline
reseed, but uses `QuickClean` after successful grayscale rendering unless the
caller explicitly requests `FullClean`. Failed grayscale updates still force
full cleaning. The following monochrome update can use differential refresh.
The fifteen-update cleaning cadence is unchanged.

Host bus-transcript tests check the quick-clean command, complete baseline
writes, the subsequent differential update, explicit full-clean requests,
and failed-grayscale recovery.

## Measured on the installed reader

The candidate was written to app1 with exact readback and byte-for-byte
preservation outside the reviewed sectors. On a warm Coffee page with an
image, four consecutive image → text transitions applied `quick-clean` at
2,087–2,089 ms. The same transition applied `full-clean` at 4,170 ms before
the change. Text turns after the quick clean stayed differential, so the
monochrome baseline is valid. Text → image grayscale rendering measured
1,761 ms in both builds.

These are protocol timings with CLI overhead, not video. A quick clean is
still a visible panel flash. Sleep, wake, battery and long-idle behavior were
not re-measured after this change.

## Panel verdict

On 2026-09-30 the reader was driven to Coffee chapter 19 and cycled between a
page with an illustration and the text page beside it three times, ending on
the text page. The reader's verdict was the faint case: an afterimage where the
picture had been that is not legible and clears within a page turn or two.

That keeps the quick clean. Making the afterimage stronger or longer-lived
would justify a full clean on the turn that leaves an image page, which costs
4,170 ms instead of 2,088 ms on exactly those turns.
