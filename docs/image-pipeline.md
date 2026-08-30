# Image pipeline

Brewthink converts source images into the SSD1677's packed 1-bit format at build time. The decoder never runs on the ESP32-C3.

## Build an image

```bash
scripts/build-image-app1.sh \
  ~/Downloads/anime-girl.jpeg \
  artifacts/anime-girl-app1.bin
```

The command creates two ignored artifacts:

- `artifacts/anime-girl-app1.bin` is an ESP-IDF app image for `app1`.
- `artifacts/anime-girl-app1.pbm` is the exact logical 1-bit frame for previewing.

It does not write hardware. Flashing still uses the guarded app1 write/readback script after its ranges have been reviewed.

## Supported input and rendering

The host decoder accepts JPEG, PNG, BMP, and PNM. It detects the format from the file contents.

The renderer performs these steps:

1. Decode to RGB8 on the build host.
2. Preserve aspect ratio with `contain` or `cover` scaling.
3. Resample with bilinear interpolation.
4. Convert RGB to integer BT.601 luma.
5. Convert luma to one bit with a 4 × 4 ordered dither or a fixed threshold.
6. Embed the exact 48,000-byte logical frame in firmware flash.
7. Apply the selected display rotation while streaming to the SSD1677.

Defaults:

```text
rotation = 270
scale    = contain
dither   = ordered
```

Override them when building:

```bash
BREWTHINK_DISPLAY_ROTATION=270 \
BREWTHINK_IMAGE_SCALE=cover \
BREWTHINK_IMAGE_DITHER=threshold \
  scripts/build-image-app1.sh input.png artifacts/image-app1.bin
```

Rotation accepts `0`, `90`, `180`, or `270`. Scale accepts `contain` or `cover`. Dither accepts `ordered` or `threshold`.

## Memory boundary

A 720 × 720 RGB8 decode needs 1,555,200 bytes before decoder overhead. The X4 has 400 KB SRAM and no PSRAM. Brewthink therefore keeps JPEG and PNG decoding on the host for this milestone.

The firmware stores only the packed 48,000-byte frame in mapped flash. It uses the existing 256-byte transfer buffer during display writes. Runtime SD-card decoding remains separate work and will require a decoder that can downscale or emit rows without allocating a full RGB image.

## Verified sample

`anime-girl.jpeg` is a progressive 720 × 720 JPEG. The pipeline decoded it into a centered 480 × 480 image inside the 480 × 800 portrait frame. A PNG encoding of the same source also completed the pipeline. Both outputs contained 101,698 black pixels and 282,302 white pixels.

The guarded workflow wrote and read back the JPEG image only in `app1`:

```text
image size:   99,552 bytes
write range:  0x650000..0x6684DF
SHA-256:      66b76caf888a0b6c6239f516a006239f0d44dfd55bf98ebafcb672cc3fc18a25
```

The write operation's built-in verification passed. Its immediate readback connection timed out. A separate read-only retry returned the exact image SHA-256. The firmware then completed one full image refresh and held without retry. `otadata` was unchanged.
