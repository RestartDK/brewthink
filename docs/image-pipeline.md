# Image pipeline

The reader preserves four logical shades through JPEG/PNG decoding, covers, previews, image viewing, and sleep. `PixelDepth::Four` uses 2 bits per pixel, not four bits. Eight-tone output remains a separate diagnostic experiment, not a reader mode.

Shared raster types and RGB rendering live in `src/image/`. Bounded embedded decoding lives in `src/image_decoder.rs`. The legacy build-time image diagnostic still accepts only a 48,000-byte monochrome asset.

## Packed reader frames

`PackedBitmap` and `PackedImage` carry dimensions, `PixelDepth`, and an exact-length buffer. Width must be divisible by eight. A four-shade 480 × 800 frame contains 96,000 bytes. Each 48,000-byte plane is row-major, with the leftmost pixel in the most significant bit. The least-significant tone plane comes first. Level 0 is black; level 3 is white. PNG previews use logical lumas 0, 85, 170, and 255, not calibrated optical reflectance.

`Gray8` draw targets retain intermediate image tones. Text, borders, and selection geometry use only black and white. Normal image paths use `Dither::None`; explicit monochrome rendering retains the ordered and threshold options.

`BufferedDisplay::refresh_image` maps four logical levels to SSD1677 states `3 - level` and runs one fixed stock absolute waveform at 20 MHz SPI. It then sleeps the controller. A binary-only frame uses the existing monochrome path at 40 MHz. After grayscale, the next binary refresh resets and fully cleans before differential updates resume. Callers cannot supply waveform bytes or arbitrary drive settings.

## Screenshots

The reader sends this header, the exact binary payload, and the terminal screen response:

```text
BREWCTL/1 SCREEN width=480 height=800 bytes=96000 crc32=<hex> bpp=2 encoding=planar
```

`device-control screen` checks dimensions, packing, payload length, CRC, and terminal success before it writes a PNG. It also accepts the legacy 48,000-byte monochrome response without depth fields. Unsupported depths and encodings fail. Host connections set raw terminal mode for binary transfers and restore the saved settings on normal exit.

These screenshots prove the intended framebuffer, not what the physical panel displayed. The integrated reader has not been flashed. See [grayscale depth investigation](grayscale-depth.md) for the separate photographed experiments.

## Prepare a four-shade host image

Build the host tool without flashing:

```bash
cargo build --target "$(rustc -vV | awk '/^host:/ {print $2}')" \
  --features host-image-tools --bin prepare-image
```

The binary accepts positional input, packed output, preview, dimensions, scale, and quantizer arguments. For example:

```text
prepare-image input.png frame.bin preview.pgm 480 800 contain gray4
```

`gray4` produces a 96,000-byte planar frame and a grayscale PGM preview. Host RGBA input composites onto white. `ordered` and `threshold` produce one-bit frames and PBM previews. A `gray4` payload cannot be embedded through the legacy binary diagnostic below.

## Build a monochrome diagnostic image

```bash
scripts/build-image-app1.sh \
  ~/Downloads/anime-girl.jpeg \
  artifacts/anime-girl-app1.bin
```

The command creates three ignored artifacts:

- `artifacts/anime-girl-app1.frame.bin` is the exact 48,000-byte firmware frame.
- `artifacts/anime-girl-app1.pbm` is the same logical frame using PBM bit semantics for previewing.
- `artifacts/anime-girl-app1.bin` is an ESP-IDF app image for `app1`.

It does not write hardware. Flashing still uses the guarded app1 write/readback script after its ranges have been reviewed.

## Supported input and rendering

The host-only `prepare-image` binary accepts JPEG, PNG, BMP, and PNM. It detects the format from the file contents. The portable `brewthink::image` module performs scaling and tone conversion for host, WASM, and firmware callers.

The runtime decoder accepts JPEG and non-interlaced PNG. It detects the format from magic bytes, enforces a 1,536-pixel dimension and 1,572,864-pixel work limit, composites PNG alpha onto white, scales with `contain` or `cover`, and quantizes to the target depth. Reader images use four shades without spatial dithering. EPUB covers, the Files image viewer, and custom sleep images call this shared decoder with different scaling policies.

The preparation and firmware build perform these steps:

1. Decode to RGBA8 on the build host and composite onto white.
2. Preserve aspect ratio with `contain` or `cover` scaling.
3. Resample with bilinear interpolation.
4. Convert RGB to integer luma with weights 54, 183, and 19, divided by 256.
5. Convert luma to one bit with a 4 × 4 ordered dither or a fixed threshold.
6. Write the exact 48,000-byte logical frame as a local artifact.
7. Validate and embed that packed frame in firmware flash.
8. Apply the selected display rotation while streaming to the SSD1677.

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

`src/bin/prepare-image.rs` owns host file decoding. `src/image/` owns portable rendering. `build.rs` retains ESP linker setup and validates that the prepared frame is exactly 48,000 bytes before copying it into Cargo's generated output directory.

## Memory boundary

A 720 × 720 RGB8 decode needs 1,555,200 bytes before decoder overhead. The X4 has 400 KB SRAM and no PSRAM, so runtime decoding never allocates a full RGB image.

PNG emits pixels from bounded deflate and scanline workspaces. JPEG emits grayscale blocks from a bounded decoder workspace. Both write directly into the packed destination. The reader retains one 96,000-byte logical frame and an 11,616-byte full cover. Publication parsing and image codecs reuse the frame allocation before composition. Catalogs, page layout, and the selected cover share mutually exclusive scratch. Three half-resolution shelf covers occupy the resource tail beyond the encoded-cover limit.

Full image decoding reads at most 96 KiB of encoded data into the resource buffer and uses its tail for decoder scratch. Encoded covers remain bounded to 128 KiB; chapter resources remain bounded to 140 KiB. No file limits were reduced for this change.

`Scratch` tracks byte and typed use. A return from typed use initializes every byte before exposing a byte slice, including any former padding. `DeviceEpub` borrows caller-owned publication storage, and `FatStorage::scan_into` fills the supplied catalog without large value returns.

The release stack check reports 12,304 bytes required, including an 8,192-byte reserve, and 25,984 bytes available. This is a check of selected compiled frames, not whole-program stack analysis.

## Verification and remaining limits

Synthetic PNG/JPEG ramps test exact four-level preservation and uniform patch interiors. Controller tests check all transmitted pixels in every rotation and the grayscale-to-monochrome baseline transition. Browser tests count four tones through image, cover, and sleep paths. Pseudo-terminal tests run the actual screenshot and image-preparation executables without connecting to hardware.

The embedded source-pixel shrinking algorithm remains phase-dependent. This PR removes binary dithering from reader images; it does not fix every resampling defect or establish optical repeatability.

## Historical monochrome sample

`anime-girl.jpeg` is a progressive 720 × 720 JPEG. The pipeline decoded it into a centered 480 × 480 image inside the 480 × 800 portrait frame. A PNG encoding of the same source also completed the pipeline. Both outputs contained 101,698 black pixels and 282,302 white pixels.

The guarded workflow wrote and read back the JPEG image only in `app1`:

```text
image size:   99,552 bytes
write range:  0x650000..0x6684DF
SHA-256:      66b76caf888a0b6c6239f516a006239f0d44dfd55bf98ebafcb672cc3fc18a25
```

The write operation's built-in verification passed. Its immediate readback connection timed out. A separate read-only retry returned the exact image SHA-256. The firmware then completed one full image refresh and held without retry. `otadata` was unchanged.
