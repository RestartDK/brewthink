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

## Cold measurement on the reader

`scripts/device-control.sh drop-image-cache` deletes the prepared pixels in
`/brew/cache`. It keeps the eviction cursor and unrelated files, and reports
how many entries it removed. Without it, every reachable illustration is a
warm cache hit and the decoder cannot be measured at all.

The same illustration was timed cold three times per build: Coffee, chapter
19, a 453 x 414 source rendered to 440 x 402. The cache was dropped before
each sample, and only the image preparation time is compared.

| Build | Cold preparation (ms) | Median |
| --- | --- | ---: |
| Before, wide-integer math | 2920, 3030, 3501 | 3030 |
| After, quotient-remainder math | 2174, 2732, 2923 | 2732 |

The device effect is roughly 300 ms on this illustration, about ten percent.
The three-sample ranges overlap, so treat the size as indicative rather than
settled. The host benchmark isolates the scaler at 19106 to 7478 us because
the rest of the cold path, extraction, JPEG decode, and the cache write,
dominates on the reader. An earlier single sample of this same image on the
pre-change build measured 3169 ms, consistent with the before median.

The interpolation stays inside validated dimensions, so the per-axis
`position * quotient + position * remainder / source` products fit native
unsigned integers even on RV32. No new buffer is allocated, and the emitted
pixels are identical to the wide-integer path under the tests above.
