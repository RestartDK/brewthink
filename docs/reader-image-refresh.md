# Returning from grayscale to text

On the installed reader, leaving a grayscale page selected a monochrome
`full-clean` refresh lasting 4,171 ms. Returning from another image to the
library selected the same mode and took 4,172 ms. These are individual driver
measurements, not optical recordings or end-to-end page-turn times.

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
and failed-grayscale recovery. They do not establish optical quality.

Before accepting this change on hardware, repeat text → grayscale → text
transitions, inspect ghosting and contrast, compare refresh timing, and verify
sleep/wake and forced full cleaning. Firmware installation requires a fresh
source-bound proof and reviewed app1 write. No post-change optical result is
claimed here.
