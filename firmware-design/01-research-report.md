# 01 — Research report: what the evidence says

A synthesis of the verified deep-research run (Appendix A, 10 findings, 21 confirmed and 4 refuted claims) and the four targeted follow-up surveys (Appendix B). Confidence labels: **high** = multiple primary sources agree; **medium** = single or new source; **verify** = must be checked on hardware.

## 1. The device is an ESP32-C3 with no PSRAM (high)

The X3 and X4 share an ESP32-C3 (single RISC-V core, 160 MHz, 400 KB SRAM of which ~380 KB is usable, 16 MB flash, Wi-Fi and BLE). The X4 Pro and other S3-based readers have 8 MB PSRAM and are a different class. Every firmware that works on the C3 is built around caching parsed content on the SD card instead of holding it in RAM. This is the single constraint that shapes the architecture in 03.

## 2. The X3's panel is a 792 × 528 UC8253 or UC8279 (high, with a verify)

The X3 has a 3.68-inch 792 × 528 panel at ~259 PPI with 4-level greyscale capability. Two controller revisions exist on identical hardware: UC8253 in original units and UC8279d_B in units shipped since about July 2026. The two need different init sequences, different SPI speeds, and the UC8279 needs waveforms uploaded by the host. One reverse-engineering gist says "SSD1677", which is the X4's controller and contradicts all driver sources. Runtime detection is mandatory and the timings (full ~470 ms, DU ~380 ms) come from papyrix's LUT documentation, not from a vendor datasheet. Confirm on the target unit.

## 3. The whole feature envelope is already proven on this hardware in C++ (high)

CrossPoint Reader (MIT, ~7.8k stars, v1.6.0 in September 2026) and its forks (CrossInk, papyrix, Witch Reader) do on-device EPUB 2/3 with hyphenation, kerning, images, footnotes, TOC, bookmarks, dictionary (StarDict), KOReader sync, and a full wireless stack: web upload UI with WebSocket uploads, WebDAV, OPDS with saved servers, Calibre wireless, and OTA from GitHub. Community app packs add Sudoku, 2048, Minesweeper, Chess, Gomoku, RSS, Wikipedia, weather, flashcards, and a Tamagotchi. Nothing in the feature catalog (05) is speculative about the hardware; the novelty of this project is doing it in Rust, from scratch, with a better transfer UX and a converter for the heavy formats.

## 4. Two Rust readers exist for the X4, proving both architectures (high for patterns, refuted for details)

pulp-os (esp-hal 1.0 + Embassy, no_std) parses EPUB on the device with a streaming ZIP/OPF/HTML-strip pipeline and a chapter cache on SD inside a ~140 to 172 KB heap, bakes fonts to bitmaps at build time with fontdue, and uses a 3-phase partial refresh that keeps reading keys during the ~400 ms BUSY period. TernOS moves parsing and layout to a desktop tool that emits a pre-rendered `.trbk` book which the device streams page by page. Claims about their exact toolchains (esp-rtos, espflash, "no dynamic dispatch") were refuted in verification, and pulp-os's author calls it in development with no install guide. Use them as pattern proofs, not as bases. This project combines both patterns: on-device for EPUB-class formats, converter for the rest.

## 5. The Rust ecosystem has the pieces, with three notable finds (medium to high)

- **hypher** gives no_std hyphenation with embedded patterns, removing the biggest typography gap.
- **exfat-slim** (weeks old) gives a no_std exFAT implementation, which matters because cards above 32 GB ship exFAT.
- **epdsi** (very new, single author) covers both UC8253 and SSD1677 with paged rendering; use as reference or fallback behind a trait.
- The radio: esp-radio needs roughly 100 to 128 KB of heap for Wi-Fi and ~200 KB with BLE coexistence. That is why Transfer and Read are exclusive modes and BLE is off by default.
- Nothing realistic exists for PDF, DjVu, DOCX, RTF, or CBR on the device. quick-xml is std-only; xmlparser and roxmltree are the no_std XML choices. rawzip plus miniz_oxide is the streaming ZIP path. Image decoders (zune-jpeg, zune-png) decode full frames, so images must be handled at ingest with bounded sizes or by the converter.

## 6. Wireless transfer: the browser is the app (high)

BooxDrop's device-hosted web page is the most-loved flow in the market. On iOS there is no Web Share Target, no Web Bluetooth, and Files cannot mount WebDAV without a third-party app, but Safari resolves `.local`, is exempt from the local-network prompt, and a Shortcut can POST a file from the share sheet. On Android an installed PWA can be a share target and `.local` resolves from Android 13. The C3 can do ~20 Mbit/s TCP and ~1 MB/s SD writes, so a streamed PUT with ranges should reach 500 KB/s or better; CrossPoint's 4 KB buffers show how easy it is to get 20× less. USB mass storage is impossible on the C3 (no OTG). BLE from an iPhone is 30 to 80 KB/s and needs a native app. See 06.

## 7. E-ink UX rules are consistent across every source (high)

Treat the screen as print: no animation, no scrolling, page-based lists, partial refresh for turns with a periodic full refresh, focus by inversion, thick strokes, 1-bit first with dithered images, minimal modals, a single-line footer, screen-tuned serif book fonts, and a crafted sleep screen. Button-only devices need a small, universal key model with long-press for secondary actions and remappable keys. See 07.

## 8. What remains unverified

1. Exact X3 key ladder voltages and the physical layout (the developer menu in milestone 0 resolves this).
2. Heap remaining under esp-radio with a TLS connection open (milestone 2 measurement).
3. 4-grey LUTs for UC8253/UC8279 in a Rust driver (milestone 0 spike; papyrix documents them).
4. Deep-sleep GPIO wake on the C3 through esp-hal (the module docs list GPIO as light-sleep only; verify RTC-IO wake on GPIO3).
5. Stock firmware bootloader details beyond "otadata + dual OTA, honoured".
