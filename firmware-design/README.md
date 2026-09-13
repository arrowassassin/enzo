# Quire — open-source Rust firmware for the Xteink X3 (design package)

Working name: **Quire** (a gathering of folded pages). Rename freely; the documents use it consistently so a search-and-replace works.

This folder is the research and design package for a from-scratch, open-source e-reader firmware written in Rust, targeting the Xteink X3 first (ESP32-C3, 792 × 528 e-ink, seven keys, Wi-Fi, microSD). It is written for three readers: the firmware author, the UI designer (Claude Design), and future contributors. Nothing in the rest of this repository is related; the previous project's code is to be removed when firmware work starts.

## Read in this order

| File | What it is |
|---|---|
| [01-research-report.md](01-research-report.md) | What the evidence says, with confidence levels and what is still unverified |
| [02-hardware.md](02-hardware.md) | X3 fact sheet: SoC, panel and both controller revisions, pin map, peripherals, power, flashing, USB lock, quirks |
| [03-architecture.md](03-architecture.md) | Firmware architecture: no_std stack, exclusive modes, memory budget, crate layout, book pipeline, `.qbk` format, display and power, CI, risks |
| [04-format-support-matrix.md](04-format-support-matrix.md) | Every reading format, on-device vs converter, tier, limits |
| [05-feature-catalog.md](05-feature-catalog.md) | The full feature set in tiers T0 to T3, including apps and games |
| [06-sideloading.md](06-sideloading.md) | Wireless transfer design: Drop page, iOS Shortcut, Android PWA, OPDS, Calibre, WebDAV, onboarding, security, acceptance tests |
| [07-ux-design-brief.md](07-ux-design-brief.md) | **The Claude Design hand-off** (also published as a page: <https://claude.ai/code/artifact/6d230cda-6d70-465f-8f0c-0bc566831401>). Section 0 is the paste-in instruction block; the rest is the key model, visual language, type scale, component library, screen inventory, flows, copy |
| [09-bookshop.md](09-bookshop.md) | Built-in free library: source evaluation, shelf index and offline catalog architecture, device UX, etiquette |
| [08-roadmap.md](08-roadmap.md) | Milestones 0 to 5 and the immediate next steps |
| [A-research-findings.md](A-research-findings.md) | Appendix A: the verified deep-research findings with sources, confidence, refuted claims |
| B1 to B5 | Appendix B: the five targeted survey reports (hardware, Rust crates, sideloading, features and UX, free catalogs), verbatim with links |

## The eleven decisions in one screen

1. ESP32-C3, no PSRAM, ~380 KB RAM: **Read, Transfer and Sleep are exclusive modes**; the radio only exists in Transfer.
2. no_std Rust: esp-hal + esp-rtos + Embassy + esp-alloc; esp-idf-svc is the documented fallback.
3. Books are **ingested once** into a compact token stream on the SD card; pages are laid out on demand and indexed per typography profile.
4. EPUB, TXT, Markdown, HTML, FB2, CBZ, MOBI on-device; **everything else through a converter** (browser WASM and CLI) into `.qbk`, which the device streams.
5. Fonts are baked to bitmap strikes at build time; user TTFs are rasterised on-device once and cached.
6. Own display driver with runtime detection of UC8253 vs UC8279d, DU/GC/4-grey waveforms, panel power-down when idle.
7. A built-in **Bookshop** browses Gutenberg, Standard Ebooks and the Palace Bookshelf from a project-hosted shelf index and an offline catalog on SD, so the device fills itself with free books without a computer.
8. Sideloading is a **device-hosted Drop page** with two QR codes, streamed resumable uploads, plus an iOS Shortcut, an Android PWA share target, OPDS, Calibre wireless and WebDAV. No app required, ever.
9. OTA from SD and from GitHub plus a recovery slot from day one, because USB-locked units exist.
10. UI is print-like 1-bit, page-based, key-driven, with inversion as focus; the design brief in 07 encodes this.
11. Everything portable compiles and tests on the host, and a desktop simulator renders the real framebuffer.

## Licence

Intended: MIT or Apache-2.0 dual licence for code, OFL for bundled fonts. The current repository LICENSE is MIT.
