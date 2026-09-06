# X4 grayscale depth investigation

Status on 2026-09-06: the selected eight recipes passed a frozen separation check in a new photographed render, with two shuffled copies of every tone. This demonstrates experimental eight-tone output under the tested conditions. Some midtones remain mottled, and the two lightest tones have the narrowest separation. Sixteen distinct tones, general reliability, and maximum panel depth are not established. Diagnostic version 3 is installed in app1.

## Question and metric

The question is how many distinct, repeatable shades the fitted GDEQ0426T82 panel can hold without spatial dithering. The metric is measured shade count, not framebuffer bit depth or the number of values a decoder accepts.

| Requested bits per pixel | Requested shades | Packed 480 × 800 image bytes |
| --- | ---: | ---: |
| 1 | 2 | 48,000 |
| 2 | 4 | 96,000 |
| 3 | 8 | 144,000 |
| 4 | 16 | 192,000 |
| 8 | 256 | 384,000 |

These are payload sizes, not firmware RAM budgets or supported display modes. A multipass implementation could stream or reuse buffers instead of retaining the whole payload.

A dithered black-and-white image can have many average tones across a region. That does not establish more than two physical shades per pixel.

## Findings from the specifications

### The panel specification guarantees black and white

The Good Display GDEQ0426T82 specification, revision 1.0 dated 2023-02-15, describes an 800 × 480 panel that displays “1-bit black, white images depending on the associated lookup table used.” See section 1 on printed page 4.

That statement does not prove that experimentally driven intermediate shades are impossible. It also does not guarantee four, eight, or sixteen shades on this physical panel.

### Two image RAM bits do not provide a selectable bit-depth setting

SSD1677 revision 1.0, section 6.5 on printed page 15, specifies two image RAM banks. Each bank holds one bit per pixel. Commands `0x24` and `0x26` write the black-and-white and red banks.

The pair of bits has four combinations. Table 6-5 maps those combinations to black or white with the documented monochrome waveforms. Four combinations are not automatically four gray shades.

There is no third image RAM plane in that documented arrangement. Packing three or four bits per pixel into an application buffer does not create an eight-shade or sixteen-shade controller mode. Sending extra bytes to a RAM-write command advances the RAM address; it does not increase pixel depth.

### Waveforms, not extra payload bits, control pigment movement

SSD1677 section 6.7 on printed page 17 describes the waveform lookup table. Command `0x32` writes waveform bytes 0 through 104. Gate, source, and VCOM settings use separate registers.

A different waveform can produce different pigment states. More shades beyond the four bit combinations would require another addressing or driving technique, such as multiple selective refresh passes. Its achievable shade count, repeatability, speed, and safety require separate evidence.

This is a limit of the documented single RAM-state selection, not a proof that four is the absolute physical maximum under every possible sequence.

### Hardware dithering is not additional physical grayscale

Commands `0x25` and `0x4D` operate the controller's dithering engine. The datasheet calls this a black-and-white monochrome feature. Accepting tone data for dithering does not establish independently driven gray pixels.

## Brewthink baseline

At source revision `b7690dc`, the image decoder writes a `MonochromeImage`. The default conversion uses a 4 × 4 Bayer pattern. The display accepts one 48,000-byte monochrome frame and supports full-clean, quick-clean, and differential refreshes.

For differential refreshes, the second controller RAM bank holds the previous monochrome frame. It is not currently a second grayscale bitplane.

Relevant implementation paths are:

- [`src/image_decoder.rs`](../src/image_decoder.rs), decoding and monochrome conversion.
- [`src/image/mod.rs`](../src/image/mod.rs), `MonochromeImage` and `RenderOptions`.
- [`src/display/ssd1677.rs`](../src/display/ssd1677.rs), RAM writes and refresh sequences.
- [`tools/device-control.rs`](../tools/device-control.rs), available USB commands.

The normal reader USB protocol has no grayscale experiment or waveform-selection command. Its `screen` command returns the intended monochrome framebuffer, not an optical measurement of the panel. The separate bench below uses its own protocol and does not run the reader app.

## Experiment record

The initial preflight found no connected reader. After the user connected it and authorized continuation, normal reader status and framebuffer capture succeeded. A readback of the current app1 passed image checksum and validation-hash checks. It differs from the older reader artifact named in `checkpoint.md`, so the live readback is the restoration source.

The diagnostic image was built, checked, written through the guarded app1-only script, and read back byte-for-byte. It does not refresh on boot. Each explicit command runs one pattern and then puts the display controller into deep sleep while the ESP32 remains available over USB.

| Attempt | Candidate | Controller result | Elapsed time | Optical result |
| --- | --- | --- | ---: | --- |
| 1 | Black-and-white reference | Completed; both chip selects high | 4,070 ms | No photograph received |
| 2 | Four raw RAM states, stock-derived absolute waveform | Completed; both chip selects high | 1,511 ms | Four ordered tones visible in `IMG_1619.HEIC`; one physical photograph |
| 3 | Four-state reference after installing diagnostic v2 | Completed; both chip selects high | 1,523 ms | No new photograph received |
| 4 | Sixteen combinations of absolute state and stock adjustment state | Both fixed passes completed; both chip selects high | 2,036 ms | `IMG_1621.HEIC` supports an eight-tone candidate; several recipes are near-duplicates |
| 5 | Four-state conditioning reference on diagnostic v3 | Completed; both chip selects high | 1,530 ms | No new photograph received |
| 6 | Frozen eight recipes, two shuffled copies each | Both fixed passes completed; both chip selects high | 2,245 ms | `IMG_1622.HEIC` passes the frozen eight-tone separation check |

Elapsed time covers initialization, RAM transfer, refresh completion, and display sleep. Each value is one observation, not a median or an isolated waveform duration. Attempt numbers in this table span all firmware versions. The final version 3 USB status reports two attempts in that boot, the probe consumed on its second attempt, and no latched fault. No SD writes occurred.

Attempt 2 now has photographic evidence of four distinct tones. It passes a visual check, not the proposed repeatability criterion below. "Not tested" is not "unsupported." A completed BUSY wait alone does not prove that the requested shades appeared.

### First physical photograph

The user supplied `IMG_1619.HEIC` and reported that the pattern works as expected. Inspection confirms the expected shade ordering in all four patch rows. The white patches blend into the white background. Each of the other three codes produces a visibly different tone.

A local script rectifies the photographed screen to 480 × 800 and samples the central half of each patch on each axis. Each sample covers 44 × 52 pixels. It calculates `0.2126 R + 0.7152 G + 0.0722 B` on the converted image's 8-bit encoded RGB values. The table summarizes the four spatial patches for each raw state.

| Raw state | Observed tone | Median of patch medians | Range of patch medians |
| --- | --- | ---: | ---: |
| `00` | White | 224.1 | 223.5 to 227.1 |
| `01` | Light gray | 206.4 | 204.5 to 212.1 |
| `10` | Medium gray | 148.9 | 145.7 to 158.7 |
| `11` | Darkest | 89.2 | 81.7 to 95.2 |

These are descriptive photo brightness values on a 0 to 255 scale, not calibrated reflectance or equally spaced gray levels. The four groups of patch medians do not overlap. Lighting varies across the photo, and capture controls were not established. The four spatial copies of a shade are not four independent refreshes.

This photograph demonstrates four-tone output in one trial. It does not establish temporal repeatability, calibrated contrast, ghosting performance, or an upper limit of four shades. No new firmware write or display command was needed to analyze it.

## Two-pass probe for higher depth

The next candidate tests whether pigment position retained between refreshes creates additional tones. It applies the stock absolute waveform, then the stock grayscale-adjustment waveform once. Both 110-byte records match the private stock backup byte-for-byte. Their frame counts, frame rates, and five electrical-setting bytes are unchanged.

The probe uses the existing patch positions. Columns choose the absolute input state. Rows choose the adjustment input state. Each cell therefore receives one of sixteen distinct input pairs. The resulting tones need not be distinct or ordered.

```text
                          Absolute input state
                          00       01       10       11
Adjustment state 00      (0,0)    (1,0)    (2,0)    (3,0)
Adjustment state 01      (0,1)    (1,1)    (2,1)    (3,1)
Adjustment state 10      (0,2)    (1,2)    (2,2)    (3,2)
Adjustment state 11      (0,3)    (1,3)    (2,3)    (3,3)
```

The first pass writes both planes and uses `0x22=0xC7`. After BUSY clears, the second pass rewrites both planes and uses `0x22=0xCF`. SSD1677 revision 1.0, printed page 28, documents both as sequences that enable clock and analog power, display, then disable analog power and clock. `0xCF` selects display mode 2 without loading a different LUT. Successful completion ends in display deep sleep.

The factory record contains 50 waveform frames. The adjustment record contains 12 at its own unchanged frame-rate settings. Background and header patches receive adjustment code `00`, the stock waveform's preservation input. Their physical preservation is part of the photo check, not assumed from that code alone.

The experimental change is applying the adjustment to gray starting tones instead of its usual monochrome starting tones. Stock bytes do not certify this new composition or its long-term behavior. This trial is bounded to one probe invocation per ESP32 boot. There is no timing or voltage sweep, no repeated adjustment loop, and no automatic paint on boot. A failed first pass prevents the second pass. Any paint error blocks further paints until reboot.

The probe completed electrically, and the user supplied `IMG_1621.HEIC`. The top patch row provides four starting-tone controls. Analysis uses the new recipe map, not the permuted four-state map from `photo-1619/analyze.py`.

### Transition-probe photograph

The photograph contains additional intermediate tones and several near-duplicates. It supports selecting eight recipes for further validation. It does not resolve all sixteen recipes into distinct tones. No separate calibrated eight-shade ramp or repeated render series has been tested.

The table gives each patch's median encoded-RGB brightness on the same descriptive 0 to 255 scale used for the first photo. Rows select adjustment state, and columns select absolute starting state. Capture conditions differ between the two photos, so their raw brightness values must not be compared directly.

| Adjustment state | Base `00` | Base `01` | Base `10` | Base `11` |
| --- | ---: | ---: | ---: | ---: |
| `00` | 182.6 | 151.8 | 81.1 | 31.8 |
| `01` | 128.8 | 102.2 | 58.3 | 32.9 |
| `10` | 184.1 | 174.5 | 163.7 | 113.6 |
| `11` | 181.5 | 163.7 | 143.6 | 71.2 |

Examples of near-duplicates, written as `(base, adjustment)`, are:

- `(3,0)` and `(3,1)`, with medians 31.8 and 32.9 and overlapping pixel-brightness ranges.
- `(1,3)` and `(2,2)`, both with median 163.7.
- `(0,0)`, `(0,2)`, and `(0,3)`, all close to the white background.

The script retains the same 44 × 52 central patch samples. It also records pixel P10 and P90 values and divides them by nearby white-strip brightness to inspect lighting effects. These ratios are not calibrated reflectance. Values slightly above one reflect imperfect local white normalization. Pixel percentiles describe spatial texture and camera variation, not temporal repeatability or confidence intervals.

An exploratory search selects eight recipes by maximizing the smallest gap between neighboring normalized P10-to-P90 ranges. The dark-to-light candidate is:

```text
(base, adjustment): (3,0) (2,1) (2,0) (1,1) (0,1) (1,0) (1,2) (0,0)
Photo median:        31.8  58.3  81.1 102.2 128.8 151.8 174.5 182.6
```

Those eight ranges are separated in this photo. The smallest normalized gap is 0.0414. This palette was selected after seeing the image, so it is a fitted candidate, not an independently validated eight-shade mode or a frozen acceptance test. The unmodified central patch crops form a visible eight-tone sequence without contrast enhancement.

Several adjustment-driven midtones look mottled in the photograph. The firmware sends uniform input codes, but uniform input does not guarantee uniform optical output. Panel texture, illumination, and camera processing have not been separated. Distinct mean brightness alone is not sufficient to claim good image quality.

The result is an eight-tone candidate worth retesting, not a sixteen-tone success or a panel-wide upper limit. No firmware write, reset, or display command was needed to analyze this photograph.

## Eight-tone repeat

Before this render, the eight selected recipes, their expected order, patch layout, sampling regions, and photo-check rule were frozen in `eight-repeat/plan.json`. Its checksum was recorded before flashing and matches the plan used for analysis. No recipe was reselected or reordered after the new photograph arrived.

Tone IDs 0 through 7 refer to the dark-to-light recipe list above. The repeat displays each tone twice at these positions:

```text
0  4  2  6
7  3  5  1
6  2  4  0
1  5  3  7
```

`Pattern::EightToneRepeat` changes only the input frames. It uses the same factory and adjustment waveform bytes, voltages, frame rates, and `0xC7` then `0xCF` sequence as the sixteen-recipe probe. Tests compare the complete command transcripts and every transmitted patch pixel. The same four-state conditioning command precedes the repeat, with the existing cooldown between commands.

### Frozen photo check

The check keeps the 44 × 52 central samples and four neighboring white strips from the previous analysis. Each patch's P10 and P90 brightness values are divided by its local white estimate. For each tone, the lower bound is the smaller normalized P10 across its two copies, and the upper bound is the larger normalized P90. All seven gaps between a darker tone's upper bound and the next lighter tone's lower bound must be positive.

This is a prospective within-photo separation check, not a calibrated noise model or a confidence interval. Camera and lighting controls were not established. A pass does not establish long-term stability or consistent output across temperatures and starting states.

### Repeat photograph result

The user supplied `IMG_1622.HEIC`. The frozen check passed. Both spatial copies preserve the expected eight-tone ordering, and all seven combined percentile-range gaps are positive.

| Tone | Recipe, base and adjustment | Photo median, copy A | Photo median, copy B | Combined normalized P10 to P90 |
| ---: | --- | ---: | ---: | --- |
| 0 | `(3,0)` | 46.0 | 49.0 | 0.2164 to 0.2378 |
| 1 | `(2,1)` | 81.9 | 79.3 | 0.3693 to 0.3936 |
| 2 | `(2,0)` | 104.3 | 111.3 | 0.4792 to 0.5214 |
| 3 | `(1,1)` | 132.3 | 135.7 | 0.5820 to 0.6489 |
| 4 | `(0,1)` | 157.3 | 166.6 | 0.7107 to 0.8121 |
| 5 | `(1,0)` | 196.6 | 191.6 | 0.8728 to 0.9057 |
| 6 | `(1,2)` | 219.7 | 217.9 | 0.9935 to 1.0145 |
| 7 | `(0,0)` | 229.8 | 226.8 | 1.0246 to 1.0590 |

The narrowest normalized gap is **0.0101**, between tones 6 and 7. A secondary descriptive check also finds all seven unnormalized pixel-range gaps positive. Its narrowest gap is 4.86 on the encoded 0 to 255 brightness scale. That secondary check was performed after capture and does not replace the frozen criterion.

The recipe order is reproduced in a second photographed render, but this is only one validation render after palette selection. Two spatial copies are not two independent renders. Mottling remains visible, especially in the adjustment-driven midtones. This is evidence for an experimental eight-tone palette, not a production-quality or lifetime guarantee. Sixteen distinct tones and the physical maximum remain unestablished.

## Diagnostic implementation

The `grayscale-bench` Cargo feature and diagnostic stage isolate these experiments from normal reader firmware. `Pattern::SixteenTransitions` and `Pattern::EightToneRepeat` name bounded experiments, not supported reader image modes. `ProbeLayout` owns the recipe mapping, and private `PaintPass` variants bind it to the fixed waveforms. The USB caller cannot supply LUT bytes, timing, voltage, or an arbitrary pass count.

- [`src/display/grayscale_bench.rs`](../src/display/grayscale_bench.rs) owns the patterns and fixed command sequence.
- [`src/x4/grayscale_bench.rs`](../src/x4/grayscale_bench.rs) owns explicit USB triggers, a five-second cooldown, a 32-attempt limit per boot, and fault latching. The `sixteen-probe` and `eight-repeat` commands share an additional one-attempt limit per boot, consumed before bus access. A failed paint blocks further paints until a deliberate reboot.
- [`tools/grayscale-bench.py`](../tools/grayscale-bench.py) submits one command and requires its matching completion response. It does not retry or flash.
- [`tools/test_grayscale_bench.py`](../tools/test_grayscale_bench.py) exercises the host CLI against a pseudo-terminal.

The bench uses 20 MHz display SPI. Its four-state candidate uses a fixed 110-byte waveform-and-voltage record that matches a publicly available X4 reference and occurs verbatim in this unit's private stock backup. The record is not a waveform invented by this investigation. No private firmware dump is committed.

The absolute grayscale transaction writes two 48,000-byte planes, loads 105 waveform bytes through `0x32`, and applies the fixed gate, source, and VCOM bytes. It sets `0x21` to normal and activates `0x22=0xC7` once. Reset and activation have a 10 ms settling delay before BUSY polling. Successful completion ends with `0x10=0x03`. Every new command resets and initializes the controller rather than treating grayscale RAM as a monochrome baseline.

The `four` pattern has two reference blocks above a four-by-four patch grid. At the default upright 480 × 800 orientation, each patch's raw state is shown below. A raw state is `(RAM_0x26_bit << 1) | RAM_0x24_bit`. Both two-pass patterns use the same geometry but their own recipe mappings described above.

```text
Top reference blocks:       00          11

Patch rows, left to right:
                            00  01  10  11
                            01  10  11  00
                            10  11  00  01
                            11  00  01  10
```

Patch interiors start at `x = 32 + 104 × column`, `y = 144 + 128 × row`. Each interior is 88 × 104 pixels. Every pixel inside a patch has the same raw code. There is no spatial dithering. The repeated and permuted patches help reveal uneven illumination or position-dependent panel behavior. The first photograph shows descending brightness in raw-state order `00`, `01`, `10`, `11`.

The black-and-white reference uses the same geometry, maps states 0 and 1 to black and states 2 and 3 to white, and uses the existing stock-parity monochrome full refresh.

Verification passed before each flash:

- 149 existing host library tests before adding the diagnostic.
- 156 host library tests and four CLI tests for version 1.
- 161 host library tests and five CLI tests for version 2. Added checks cover all sixteen transmitted input pairs, exactly two activations, unchanged frame budgets and voltage tails, and failures between or during passes.
- 165 host library tests and six CLI tests for version 3. Added checks cover the frozen palette, two copies per tone, unchanged command transcripts, probe classification, and aborting before the adjustment pass.
- Host and embedded Clippy with warnings denied. The normal-reader regression run still passes 149 tests.
- Formatting, whitespace checks, embedded release builds, image validation, and flash readbacks.
- Versions 2 and 3 contain the same waveform records, also matched to the private stock backup. Version 3's palette matches the earlier photo selection exactly. The restoration image checksum still matches its original review.

## Installed image and restoration

```text
Diagnostic image:   artifacts/grayscale-depth/eight-repeat/bench.bin
Diagnostic ELF:     artifacts/grayscale-depth/eight-repeat/bench.elf
Image bytes:        105840
Image SHA-256:      9024b7218bc2697def152b7a7c7d64a45aac60e3880358bb27a81eb99f3c64a5
App1 write range:   0x650000..0x669D6F
App1 sector range:  0x650000..0x669FFF
```

The running reader was preserved before the write in ignored `backup/grayscale-before/reader.bin`. That validated readback covers 503,952 bytes beginning at `0x650000`, including the entire diagnostic write and its sector boundary. Restoring it requires the guarded write/readback script and another range review. Its byte range is `0x650000..0x6CB08F`; its sector range is `0x650000..0x6CBFFF`.

The diagnostic remains installed for optical inspection. Normal reader commands are unavailable in this firmware. The host work tab and its processes were closed after the commands completed. No firmware, boot-selection, or SD changes occur merely by viewing the held pattern.

## Remaining validation

- The selected eight recipes passed one prospective repeat check, but the optical setup is not calibrated.
- The broader repeated-capture protocol and its noise threshold remain a proposal. The frozen single-photo check is narrower in scope.
- Only one validation render has followed palette selection. Mottling, ghosting, temperature dependence, and reliability across starting states remain uncharacterized.
- Sixteen distinct tones are not demonstrated by this candidate.

Further validation needs fixed lighting and camera settings, multiple independent renders from white and black starting states, and checks for mottling and ghosting. The successful repeat must not be treated as a measured panel maximum.

## Proposed measurement protocol

This protocol is a proposal. The measurement setup and acceptance threshold have not been calibrated or frozen.

The test starts with two solid black-and-white reference patches, then four, eight, and sixteen requested uniform shades where a reviewed drive sequence exists. Patterns contain no spatial dithering. Repeated runs permute patch locations to expose lighting and panel-position effects.

The camera remains fixed with manual exposure, focus, and white balance under constant, diffuse lighting. RAW capture is preferable. Black and white reference patches remain in each capture. Measurements use patch interiors and exclude borders, text, glare, and defective pixels. Panel temperature and settling time are recorded and held consistent.

A candidate needs at least five independent render-and-capture repeats from each of two controlled starting states, white and black. Each repeat uses the same documented conditioning sequence. The results include normalized optical levels, within-patch variation, between-run variation, adjacent-shade separation, refresh duration, and residual ghosting.

Before candidate testing, the measurement harness must distinguish black from white and reject duplicate patches labeled as different shades. The numerical separation threshold is then fixed from measured noise. An overlap or unstable shade is a failed candidate, not a reason to change the threshold afterward.

Each iteration changes one reviewed waveform or pass sequence. A supported candidate needs every requested shade to remain distinguishable, ordered, and repeatable within the frozen criterion. USB responsiveness, bounded BUSY waits, and recovery to the monochrome baseline remain regression gates.

The search stops when the next candidate has no reviewed safe drive sequence, fails the optical criterion after a bounded retest, or triggers a safety gate. A failed candidate does not establish that all possible waveforms at that depth fail. The final report states the highest depth demonstrated under the measured conditions and lists the actual rejected sequences.

## Safety boundary

“Breaks” means a candidate fails measurement or a bounded software check. Physical panel damage is not an experiment objective.

The investigation does not sweep supply voltages, VCOM, source or gate levels, arbitrary pulse durations, or uncontrolled repeated refreshes. Both candidates use only the fixed settings from the reviewed stock-derived records. The two-pass composition is experimental and separately bounded. Absolute maximum ratings are not normal operating targets. Any different waveform or electrical setting needs a new review.

Controller OTP programming is excluded, in addition to ESP32 eFuse programming. Tests must not alter stock app0, bootloader, partition table, boot selection, NVS, or other flash partitions. Any approved diagnostic remains confined to its reviewed app1 range.

## Evidence and references

Local evidence is ignored under `artifacts/grayscale-depth/`. The decision log is `artifacts/grayscale-depth/decision.tsv`. `bench-flash.log` records write/readback verification. `attempt-01-binary.log`, `attempt-02-four.log`, and `bench-status-after.log` record controller execution.

The `transitions/` directory contains version 2's pre-flash review, unchanged-table provenance checks, test logs, image, flash readback log, baseline result, probe result, and final USB status. `review.md` records the composition's limits. `check-provenance.py` verifies the exact waveform records and restoration checksum locally without publishing the stock dump.

The `photo-1619/` directory contains the converted physical photograph, checksums, rectified `panel.png`, and per-patch `analysis.json`. Rerun the descriptive photo analysis with `python3 artifacts/grayscale-depth/photo-1619/analyze.py`. The script uses manually selected screen corners recorded in its output. An initial crop used dimensions before orientation correction and sampled outside the panel. That result was rejected, preserved with the `invalid-initial-` prefix, and replaced only after inspecting the corrected crop. Original-photo metadata stays in ignored local artifacts.

The `photo-1621/` directory contains the second photo conversion, checksums, rectified screen, per-recipe measurements, sorted crops, and `candidate-eight.png`. The crop strip preserves the rectified photo's pixel values and only changes their order. Rerun the analysis with `python3 artifacts/grayscale-depth/photo-1621/analyze.py`. The output records the corner coordinates, recipe mapping, exploratory palette-selection rule, and limitations. Both photographs and their metadata remain ignored local evidence.

The `eight-repeat/` directory contains version 3's frozen `plan.json`, its pre-flash checksum, tests, image, and hardware logs. `evaluate.py` implements the frozen check. Its synthetic tests accept separated tones and reject duplicated tones and missing patches.

The `photo-1622/` directory contains the repeat photograph, registered screen crop, two tone-ordered crop strips, per-patch measurements, `evaluation.json`, and a secondary raw-brightness check. Rerun with `python3 artifacts/grayscale-depth/photo-1622/analyze.py`, then `python3 artifacts/grayscale-depth/eight-repeat/evaluate.py artifacts/grayscale-depth/photo-1622/analysis.json`. The analysis checks the frozen plan checksum before sampling.

- [Good Display GDEQ0426T82 specification, revision 1.0](https://www.laskakit.cz/user/related_files/gdeq0426t82.pdf), section 1 and absolute maximum ratings.
- [Solomon Systech SSD1677 specification, revision 1.0](https://files.waveshare.com/upload/2/2a/SSD1677_1.0.pdf), sections 6.5 and 6.7 and command descriptions for `0x24`, `0x25`, `0x26`, `0x32`, and `0x4D`.
- [Brewthink display bring-up](display-bringup.md).
- [Brewthink image pipeline](image-pipeline.md).
