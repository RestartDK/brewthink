# Application shell

Brewthink opens on a three-item home menu. Books is the primary path, Files exposes the readable EPUB files on the card, and Settings controls reader typography. Back returns to the parent screen. Power retains the active screen and reading location.

```text
physical or web input
        |
        v
App state machine
  Home | Books | Files | Settings | Reader | Sleeping
        |
        v
platform effect runner
        |
        v
shared 480 x 800 monochrome renderer
        |
        +--> SSD1677
        `--> web canvas
```

## State ownership

`App` owns navigation, selection, committed reader preferences, settings drafts, reading checkpoints, and the latest battery status. Platform adapters own files, EPUB bytes, display I/O, and battery sampling. Inputs mutate `App` and return typed effects. Renderers receive immutable views.

Books and Files refer to the same catalog by `BookId`. Books presents covers and metadata. Files presents source filenames and sizes. Opening either route enters the same reader and remembers which route Back should restore.

## Typography

Reader preferences use bounded choices for font, size, and line spacing. The default is Noto Serif 14 pt, matching CrossPoint Reader. One resolved reader theme supplies both pagination and rendering metrics. The fixed Brewthink wordmark and application chrome never inherit reader typography.

A reading checkpoint records the preferences used to derive its page. Reopening under different preferences maps the previous chapter progress into the new page count. Stable semantic content anchors remain the longer-term persistence target.

## Battery

The application bar reserves the upper-right corner for a battery icon and percentage. USB power removes the capacity fill and adds a lightning bolt inside the outlined battery. GPIO20 detects USB presence, not active charge current. While USB is connected, the application preserves the last battery-only percentage because charging voltage would inflate the discharge estimate. It still records the measured voltage for diagnostics. USB transitions and five-point percentage changes refresh the display; smaller changes remain cached until the next application render.

## Persistence boundary

The X4 retains the active screen, reader location, and preferences in checksummed RTC fast memory across deep sleep. The browser stores applied reader preferences in versioned local storage. Durable device writes require a separate, restricted application-data store and explicit removable-media review. No stock flash, NVS, or firmware partition is used.
