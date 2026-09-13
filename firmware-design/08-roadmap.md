# 08 — Roadmap

Milestones are ordered so that each one produces something a user can flash. Feature tiers refer to 05.

| # | Milestone | Deliverable | Retires risk |
|---|---|---|---|
| 0 | **Bring-up spike** (2 to 3 weeks) | Board crate: panel detection, both controllers refreshing a test pattern with GC and DU, 4-grey demo, key ladders with a calibration screen, fuel gauge, RTC, IMU read-out, SD read/write, deep sleep and wake, recovery app that reflashes from SD, OTA slot switch. Desktop simulator skeleton. | Display driver, USB-locked update path, heap floor |
| 1 | **Reads a book** | EPUB and TXT ingest to QTX, layout engine with hyphenation and justification, reader screen with footer, TOC, position memory, Home and Library (list view), settings for font size and margins, sleep screens. Rendering snapshot tests. | Layout quality, ingest speed |
| 2 | **Drop page** | Wi-Fi station and hotspot, mDNS, captive DNS, HTTP server with streaming PUT and multipart, the Drop page PWA, ingest in Transfer mode, OTA from GitHub, NTP time. First public alpha. | Radio heap, upload throughput |
| 3 | **Daily driver (T1 complete)** | Markdown, HTML, FB2, CBZ, image folders, `.qbk` FIXD reader, converter CLI and web app (PDF, DOCX, CBR, DjVu via CLI), dictionary, bookmarks, footnotes, WebDAV, OPDS, **Bookshop with offline catalog and the nightly shelf-index job**, Calibre wireless, KOReader sync, iOS Shortcut, statistics core, all settings screens, cover grid, orientation, auto-turn. | Converter UX |
| 4 | **Enthusiast (T2)** | MOBI/AZW3, word cursor with highlights and notes, vocabulary builder, skim, search, series and collections, typography profiles, 2-bit anti-aliasing, tilt turn, profiles, stats calendar and streaks, flashcards, news, clock, Wikipedia, custom fonts, CJK packs, localisation. | — |
| 5 | **Delight (T3)** | Games hub (Sudoku, 2048, Minesweeper, Chess first), interactive fiction, weather, calculator, notes, year in review, reading pet, ESP-NOW two-player, BLE provisioning. | — |

Working cadence: every milestone ends with a tagged release with binaries, an `update.bin` for the SD path, and the web converter deployed. Open-source hygiene from day one: MIT or Apache-2.0 dual licence for code, OFL fonts, a `CONTRIBUTING.md` that explains the simulator-first workflow, `good first issue` labels on apps and games because they are isolated crates with a tiny interface.

## Immediate next steps

1. Photograph the device, count and label the keys, and confirm the panel controller revision with the developer screen from milestone 0 (or with CrossPoint's about screen).
2. Apply for Standard Ebooks' open-source feed access (their feeds page describes the process) and verify the Palace Bookshelf OPDS 2.0 URL from a real network.
3. Decide the project name (this document set uses "Quire" as a working name; `.qbk` and `/.quire/` follow it).
4. Paste section 0 of 07 into Claude Design and generate the components board and screens 10, 11, 20, 21, 30, 40 first, since those are the screens milestone 1 and 2 need.
5. Start milestone 0 in a fresh repository layout (the current repository's code is unrelated and can be removed or archived).
