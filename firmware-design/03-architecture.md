# 03 — Firmware architecture

A from-scratch Rust firmware for the Xteink X3, designed around one fact: **380 KB of RAM, no PSRAM, and a radio stack that wants a third of it**. Everything below follows from that. Crate versions are as of 2026-09-13 (see Appendix B, section 2 for the full survey with links).

## 1. Decisions

| Decision | Choice | Why |
|---|---|---|
| Runtime | **no_std**: esp-hal 1.2 + esp-rtos 0.4 + Embassy 0.10 + esp-alloc | Smallest RAM floor, the two existing Rust readers on this hardware (pulp-os, TernOS) both chose it, single toolchain (`cargo`, `espflash`), no ESP-IDF/cmake/python build. The std path (esp-idf-svc) is the documented fallback if Wi-Fi coexistence or TLS proves unworkable. |
| Radio | esp-radio, **Wi-Fi only by default**; BLE compiled behind a feature flag | Wi-Fi + BLE coexistence on the C3 costs ~200 KB heap; Wi-Fi alone ~100 to 128 KB. BLE has no role in book transfer (see 06). |
| Modes are exclusive | The device is in exactly one of **Read**, **Transfer**, or **Sleep** | Radio memory is only allocated in Transfer. Heavy parsing (ingest) streams and runs in Transfer with a bounded budget. Reading never competes with the radio. |
| Book pipeline | **Ingest once → compact chapter token stream on SD → lay out pages on demand → page index cached per typography profile** | The pattern that made CrossPoint, pulp-os and EPub-InkPlate work on this class. Parsing cost is paid once; page turns are a render of a small token slice. |
| Heavy formats | Off-device converter (WASM in the browser and a CLI) produces `.qbk`; device streams it | PDF, DjVu, DOCX and friends have no realistic no_std parser and would not fit in RAM anyway. |
| Display | Own UC8253/UC8279 driver (epdsi as reference and fallback), 1-bit framebuffer in RAM, 4-grey path for images and anti-aliased text via a second plane | Runtime controller detection is mandatory; no crate ships the 4-grey LUTs for these controllers, so they are transcribed from papyrix docs. |
| Fonts | TTF rasterised **at build time** into 1-bit and 2-bit bitmap strikes at the 8 reader sizes and 5 UI sizes; on-device `ab_glyph` rasteriser only for user-supplied fonts, cached to SD | fontdue at runtime fully parses the font into the heap. Build-time strikes cost flash (which we have 16 MB of), not RAM. |
| UI | embedded-graphics + a small in-house immediate-mode screen system; embedded-text for wrapped UI text; embedded-menu for settings lists | No general toolkit fits a 1-bit, page-based, key-driven UI. |
| Storage | embedded-sdmmc (FAT32) plus exfat-slim behind a volume trait; settings in flash via sequential-storage + postcard | Cards over 32 GB ship exFAT. |
| OTA and recovery | ESP-IDF-compatible partition table (CrossPoint's layout), esp-bootloader-esp-idf for A/B switching, update from SD and from GitHub releases, a small recovery app in a third slot | USB-locked units can only be updated this way. |
| Testing | Every document, layout and UI crate is `no_std` + `alloc` and **compiles and tests on the host**; a desktop simulator renders the real framebuffer in a window with keyboard input | UI and format work should not need the device in hand. The simulator also produces the designer's reference PNGs. |

## 2. Memory budget

Numbers are targets to enforce with `esp-alloc` stats in the developer menu, not measurements. Total usable SRAM ≈ 380 KB.

| Region | Read mode | Transfer mode | Notes |
|---|---|---|---|
| Static (`.data`/`.bss`): drivers, UI state, task arena, Wi-Fi statics | 40 KB | 60 KB | esp-radio adds statics even before its heap |
| Stacks: main + 4 Embassy tasks + interrupt | 40 KB | 48 KB | |
| Framebuffer, 1-bit, 792 × 528 | 52 KB | 52 KB | In the reclaimed DRAM2 region. Second plane for 4-grey (52 KB) allocated only while a grey page is on screen |
| Radio heap (esp-radio + smoltcp buffers + TLS) | 0 | 128 KB (+32 KB per TLS connection) | Released completely on leaving Transfer |
| Document working set (chapter tokens, page layout, glyph cache, image decode band) | 120 KB | 40 KB | Page layout for a 26 px page is ~6 KB; a chapter token stream is streamed in 8 KB windows |
| Slack | ≥ 60 KB | ≥ 20 KB | Fragmentation headroom |

Rules that keep this honest:
- No file is ever read whole. Every parser is a streaming state machine over 4 to 16 KB windows.
- Images are decoded band by band at ingest time into pre-scaled 1-bit or 2-bit PBM/QOI files on SD; the reader only blits.
- The chapter cache, page index and thumbnails live under `/.quire/` on the SD card, so the RAM cost of a large library is zero.
- The heap is measured at every mode switch and logged; a build that regresses the Read-mode slack below 60 KB fails CI on the simulator's budget test.

## 3. Crate layout (Cargo workspace)

```
firmware/
  Cargo.toml                 workspace
  crates/
    board-x3/                pins, panel detect (UC8253 vs UC8279), key ladders, BQ27220, DS3231, QMI8658, SD rail
    epd/                     controller driver: init scripts, LUT tables (GC/DU/4-grey), DTM1/DTM2 planes, BUSY wait, 3-phase refresh
    gfx/                     Framebuffer1, Framebuffer2, dither (FS + Bayer), bitmap font strikes, glyph blitter
    layout/                  line breaking (UAX14), hyphenation (hypher), justification, page filler; input = token stream, output = page draw list
    qtx/                     chapter token stream format (text runs, style bits, anchors, image refs, footnote links)
    doc-epub/                rawzip + miniz_oxide streaming → xmlparser XHTML → qtx; OPF/NCX/nav metadata
    doc-txt/  doc-md/  doc-fb2/  doc-html/  doc-mobi/  doc-cbz/  doc-qbk/
    library/                 index file, metadata, covers, collections, positions, stats log
    dict/                    StarDict .ifo/.idx/.dict(.dz) reader
    net/                     wifi manager, http server (edge-net or picoserve), drop page API, PUT/range uploads, WebDAV, mDNS, captive DNS, OPDS client, Calibre wireless, KOReader sync, OTA client, NTP
    ui/                      screen trait, key model, widgets, screens (home, library, reader, settings, drop, stats, ...)
    apps/                    clock, flashcards, news, wiki, weather, calc, notes, images, ifiction
    games/                   sudoku, 2048, minesweeper, chess (cozy-chess + alpha-beta), checkers, gomoku, wordle, solitaire, picross, trivia
    kernel/                  modes, power manager, task wiring, event bus
  bin/
    quire-x3/                the firmware binary
    quire-recovery/          tiny recovery app: reflash from SD
  tools/
    sim/                     desktop simulator (window = framebuffer, keyboard = keys, folder = SD card)
    qbk-convert/             CLI converter (std): PDF/DjVu/DOCX/RTF/CHM/AZW3/CBR → .qbk; font pack builder
    qbk-web/                 the same converter core as WASM + a static web page
    fontpack/                build-time TTF → bitmap strike generator (used by build.rs and by users)
  web/
    drop/                    the Drop page (single HTML, inlined CSS/JS, gzipped into flash)
```

Every crate under `crates/` except `board-x3`, `epd`, `net` and `kernel` is pure `no_std + alloc` and has host tests. `doc-*` crates share a `Document` trait: `open(&mut dyn RandomRead) → Metadata`, `spine() → [ChapterRef]`, `ingest_chapter(i, &mut dyn Write /* qtx */)`, `cover(&mut dyn Write)`.

## 4. Runtime structure

Embassy tasks, all on the single core:

1. **ui** — owns the framebuffer and the screen stack. Loop: wait for an event (key, timer, transfer progress, ingest done) → update screen state → draw into the framebuffer → hand the frame to `epd`. Draw is synchronous and must finish under 60 ms for text pages (glyph blits only).
2. **epd** — owns the SPI to the panel. Implements the 3-phase refresh from pulp-os: write the new plane, kick the DU or GC waveform, keep polling keys during the ~400 ms BUSY, then sync the old plane. Chooses DU vs GC by the page counter and by "content had an image or dialog".
3. **storage** — owns the SD SPI and the filesystem. All file IO goes through a request channel so the shared SPI bus is never interleaved. Also serialises the `.quire` cache writes.
4. **ingest** — runs only in Transfer mode and only when the ui is idle: streams newly added books into chapter caches, thumbnails, and metadata; publishes progress to `ui`.
5. **net** — exists only in Transfer mode: Wi-Fi station/AP, HTTP server, WebDAV, mDNS, captive DNS, OPDS/Calibre/sync clients, OTA. Dropped entirely (task ends, heap region freed) when Transfer ends.
6. **power** — idle timer, panel power-down, light sleep between events, deep sleep after the timeout, SD rail control, fuel-gauge polling, IMU tilt events.

Key input uses the two ADC ladders sampled at 100 Hz with debouncing and long-press/hold detection in `board-x3`; the Power key is a GPIO interrupt and the deep-sleep wake source.

## 5. The book pipeline

```
 upload / SD scan
      │
      ▼
 ingest ──► metadata.json, cover.qoi, thumb.pbm            (/.quire/books/<id>/)
      │
      ├──► chapters/NN.qtx  (token stream, ~1.2× the plain text size)
      └──► images/NN.pbm    (pre-scaled to 480 px wide, 1-bit FS dither, or 2-bit)
                      │
 open book            ▼
      │      layout(profile) ──► pages/<profile-hash>/NN.idx   (byte offset of each page start)
      ▼
 page turn: seek chapter at pages[n] → stream ~4 KB of tokens → line-break + justify → draw list → blit
```

- **QTX** is our own tiny binary token format: `Text(run)`, `Style(bits: bold, italic, mono, small-caps, sup, sub)`, `Para(kind: body, heading1-3, quote, verse, list-item, code, caption)`, `Image(ref, w, h)`, `Anchor(id)`, `Link(target)`, `Footnote(id)`, `Break`. It is what every format compiles to, so the layout engine has one input.
- **Layout** is incremental: laying out chapter N page by page and recording page starts. Opening a book at 43% with a new font size lays out only the chapter containing the position (a few hundred ms), and the page index for the rest of the book fills in during idle time. Progress and time-left estimates use character counts, not page counts, so they are stable across profile changes.
- **Hyphenation** via hypher (English, German, French, Spanish, Italian, Dutch, Portuguese, Russian patterns compiled in; the user can drop more on SD). Line breaking via unicode-linebreak, justification with a Knuth-Plass-lite three-line lookahead.
- **Fonts**: bitmap strikes are generated by `fontpack` from Literata, Noto Sans, JetBrains Mono, and Atkinson Hyperlegible (all open licences) at the 8 reader and 5 UI sizes in regular, bold, italic, bold-italic. Glyph coverage: Latin, Latin Extended, Greek, Cyrillic, general punctuation, currency, arrows. Additional packs (CJK, Arabic shaping via harfrust) are SD-loadable and demand-paged through a 16 KB glyph cache.
- **Images**: at ingest, JPEG via zune-jpeg with pre-check of dimensions (huge images are decoded in bands through an MCU-restart streaming path or, when above 1600 px, skipped with a placeholder and a note that the converter can fix it), PNG via minipng (non-interlaced) with zune-png fallback, BMP via tinybmp. Output is a 1-bit or 2-bit pre-dithered PBM at final size. Covers get a 152 × 228 thumbnail and a full-screen 528 × 792 sleep version.

## 6. The `.qbk` format (converter output)

A single file, little-endian TLV sections with a table of contents at the front, no compression container required (sections may be individually deflate-compressed):

- `META` JSON: title, authors, series, index, language, tags, identifiers, source format, converter version.
- `COVR` cover image (QOI) + thumbnail.
- `FLOW` mode: `CHAP` sections holding QTX token streams, `IMG ` sections holding pre-scaled images, `TOC ` nested entries, `NOTE` footnotes. The device ingests this in seconds because the hard work is done.
- `FIXD` mode (for PDF, DjVu, scanned comics, sheet music): `PAGE` sections holding 1-bit or 2-bit 528 × 792 (or 792 × 528) bitmaps, deflate-compressed, plus optional `TEXT` per page for search and dictionary, plus a `CROP` box for margin trimming, plus an optional `RFLW` section carrying reflowed text as a FLOW chapter set when the converter could extract it (so a PDF can be read either as pages or as reflowed text).
- `FONT` optional embedded bitmap strikes for publisher fonts.

The converter core is one Rust crate reused by the CLI and the WASM web app. For PDF the CLI uses pdfium-render and the web app uses pdf.js for rasterisation; both feed the same `.qbk` writer. DjVu uses djvulibre via the CLI only. DOCX/RTF/CHM are parsed with std crates on the desktop and emitted as FLOW.

## 7. Display driver notes

- Detect the controller at boot as described in 02. Keep the SPI at 10 MHz for UC8253 and 20 MHz for UC8279.
- Waveforms: GC (full, ~470 ms) for entering the reader, dialogs, images, and every Nth page; DU (~380 ms) for page turns and focus moves; a 4-grey two-pass mode (~130 ms extra) when a page contains a 2-bit image or when anti-aliased text is on.
- Because the UC8253 has no window update, a "partial" refresh is still a full-frame transfer of 52 KB (~42 ms at 10 MHz). The saving is in the waveform, not the transfer.
- Power the panel down (deep-sleep command) whenever the screen is static for more than a few seconds; papyrix does this to stop sunlight fading.
- The 3-phase refresh keeps the key sampling alive during BUSY so a fast reader can queue the next page turn.

## 8. Power management

- **Read mode**: CPU at 160 MHz only while laying out and blitting, then light sleep with the ADC key timer as the wake source. Target: under 3 mA average while a page is displayed, wake-to-next-page under 100 ms plus the refresh.
- **Idle**: after 1 to 60 min (user setting) show the sleep screen, power down the panel, pull the SD rail low with hold, enter deep sleep. Wake on the Power key. Quick-resume restores the reader screen from the saved position with a single GC refresh.
- **Transfer**: Wi-Fi power-save on; session times out after 10 min idle.
- The BQ27220 gives state of charge and current; the current sign is the USB-present signal on the X3.

## 9. Build, flash, CI

- `cargo build --release --target riscv32imc-unknown-none-elf` with `espflash flash --monitor`. `just` recipes for `flash`, `sim`, `test`, `fontpack`, `web`, `release` (produces `quire-x3.bin` and an `update.bin` layout the recovery app and the stock SD path accept).
- CI: host tests for all portable crates, a rendering snapshot test suite (PBM per screen and per sample page), a heap-budget test in the simulator, a firmware size check, and a release job attaching binaries and the web converter.
- Corpus: a set of open-licence sample books (Standard Ebooks EPUBs, Gutenberg TXT, a CBZ from the public domain, FB2 and MOBI samples) used by the parser tests.

## 10. Risks and how they are retired

| Risk | Retire by |
|---|---|
| Panel driver: 4-grey LUTs and UC8279 init scripts are transcribed, not vendor-published | Milestone 0 spike on real hardware; keep the driver behind a trait so epdsi can be swapped in |
| esp-radio heap need leaves too little for ingest | Measure at milestone 2; fallback is to ingest only when the radio is idle, or to move all ingest to the converter |
| TLS heap (mbedtls ~32 KB per connection) | One connection at a time, OPDS and OTA only; embedded-tls (no cert verification) as a user-selectable low-RAM option |
| exfat-slim is weeks old | Ship FAT32 first; exFAT behind a feature flag with a "format to FAT32 in the Drop page" helper |
| Button ladder values differ per unit | Developer menu shows raw ADC; a calibration screen stores thresholds in flash |
| Bricking a USB-locked unit | Recovery slot + SD update path are milestone 0 deliverables, tested before any user flashes |
