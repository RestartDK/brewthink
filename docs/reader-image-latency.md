# Image scaling cost

Image page latency includes chapter lookup/preparation, extraction, decoding,
cache I/O, composition, and panel refresh. Optimizing one stage does not prove
an end-to-end improvement. In particular, warm cache hits do not run the image
decoder.

The decoder previously performed four signed 128-bit divisions per source
pixel to compute its destination rectangle. The replacement computes each
axis ratio once as a quotient and remainder:

```text
floor(position * scaled / source)
  = position * quotient + floor(position * remainder / source)
```

Validated source dimensions are at most 4,096. Therefore the remainder product
is at most 4,096 × 4,095, and the quotient product and their sum are at most the
scaled dimension. The per-pixel operations fit native unsigned integers,
including on RV32. Separate inset/crop offsets preserve the previous centered
placement and rounding. Rows with no destination pixels skip horizontal work.
No new image buffer, approximate reciprocal, or pixel-format change is used.

Tests compare destination rectangles with the original wide-integer formula
for containment, cropping, extreme aspect ratios, and up/downscaling. Axis
tests also exercise every source edge and scaled dimensions up to `usize::MAX`.
The ignored `benchmark_streamed_image_scaling` test measures CPU-only scaling
of an authored 1200 × 1574 pattern into 480 × 800 pixels and reports its CRC.
It is not an ESP32-C3 benchmark or SRAM evidence.

Before accepting this optimization as a fix for device image lag, compare
cold and warm page turns on identical source images and render settings,
with separate decode/cache and display timings. Keep originals unchanged and
run the firmware memory proof. Hardware latency improvements are not yet
claimed.
