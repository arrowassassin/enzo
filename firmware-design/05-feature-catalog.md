# 05 — Feature catalog

The complete feature set for the firmware, organised by area and tiered by release. The tiers are a build order, not a cut list: everything here is intended to ship eventually.

Tiers: **T0** boots-and-reads (first flashable build) · **T1** daily driver (replaces stock and CrossPoint for most people) · **T2** enthusiast (the reasons to pick this firmware) · **T3** delight and apps.

Feasibility notes reference the hardware sheet (02) and the format matrix (04). Where a feature is proven on this exact hardware by CrossPoint, papyrix, CrossMux, CrossPet or pulp-os, it is marked *(proven)*. Sources for the survey are in Appendix B.

## 1. Reading core

| Feature | Tier | Notes |
|---|---|---|
| EPUB 2 and 3 on-device (text, headings, emphasis, lists, block quotes, images, footnotes, TOC from NCX or nav) | T0 | *(proven)* streaming ZIP + HTML strip, chapter cache on SD |
| TXT with encoding detection (UTF-8, Latin-1, GB18030 via table on SD) | T0 | *(proven)* lazy page index |
| Pre-rendered book format (`.qbk`, our own, see 04) from the desktop/web converter for everything else | T0 | The escape hatch that makes "every format" true |
| Markdown, HTML (single file), FB2 | T1 | Same HTML-strip engine; FB2 is simple XML |
| CBZ comics and image folders (JPEG/PNG/BMP), fit-to-width, 2-bit dither, panel-by-panel zoom | T1 | Streaming decoders only, see 04 |
| MOBI / AZW3 (KF7 and KF8, DRM-free) | T2 | On-device via PalmDOC/HUFF-CDIC + HTML strip; converter fallback |
| PDF, DjVu, DOCX, RTF, CHM, AZW with DRM | via converter | See 04. PDF reflow and DjVu are not realistic on 380 KB RAM |
| Position memory per book, resume on open, "quick resume" from sleep showing last page | T0 | |
| Chapter navigation: TOC screen with nesting, prev/next chapter on long-press | T0 | *(proven)* |
| Go to: percent, page, chapter | T1 | |
| Hold-to-skim: hold Right to fan pages at ~5/s with a position bar, release to land | T2 | Kindle Page Flip for buttons |
| Footnotes and endnotes: jump, read in a popup, return | T1 | *(proven)* |
| In-book text search | T2 | Search the cached chapter text on SD |
| Bookmarks with a bookmarks list | T1 | |
| Highlights and notes: select a sentence by cursor, save; notes typed via on-screen keyboard or added later from the web UI | T2 | Export as Markdown in the web UI |
| Dictionary lookup: cursor to a word, long-Confirm; StarDict dictionaries on SD, `.syn` synonyms, HTML definitions rendered by the book engine | T1 | *(proven)* |
| Vocabulary builder: every looked-up word saved, reviewable as flashcards | T2 | |
| Wikipedia lookup (text-only, cached to SD) | T2 | Needs Wi-Fi |
| Translate a sentence (online API, user-supplied key) | T3 | |
| Auto page turn (pages per minute) | T1 | *(proven)* |
| Tilt and shake page turn via the IMU, with sensitivity setting | T2 | *(proven on stock)* |
| Reading in all four orientations, with button remap following the orientation | T1 | *(proven)* |
| **The book is the home screen**: wake lands on the page; Home is a one-press layer over it | T0 | Signature (07) |
| **Compass** quick menu with choices at the physical key positions; two-press rule for every common action | T0 | Signature (07) |
| **The Spine**: fore-edge progress strip on the reading page, doubles as the skim scrubber | T1 | Signature (07) |
| **Smart word cursor**: long-Confirm lands on the rarest word on the page first (frequency list in flash) | T2 | Two presses to a definition most of the time |
| **Jump** launcher on long-Back listing everything, filterable from the phone | T1 | Signature (07); apps register here |
| **Peek strip**: hold Back briefly for chapter, time left, clock and battery without leaving the page | T1 | |
| **Page pre-render**: next page always laid out into a shadow plane during idle so a turn only kicks the waveform | T0 | Speed budget in 07 §7 |
| Chapter openings typeset like a book: numeral, title, rule, drop cap (toggle) | T1 | |
| Series and collections, "next in series" at the end of a book | T2 | Metadata from OPF `belongs-to-collection` or Calibre |
| End-of-book screen: time taken, mark finished, rate, next in series | T2 | |

## 2. Typography

| Feature | Tier | Notes |
|---|---|---|
| Built-in fonts: one serif (Literata or Bookerly-class), one sans (Noto Sans or Inter), one mono, one dyslexia-friendly (OpenDyslexic or Atkinson Hyperlegible) | T0 | Baked to bitmaps at build time, see 03 |
| Font sizes: 8 steps (roughly 9 to 16 pt at 259 PPI) | T0 | |
| Custom fonts from SD: TTF/OTF converted by the desktop tool into our bitmap font pack; also raw TTF rasterised on-device at a fixed set of sizes (slow first time, cached) | T1 | |
| Weight: regular, bold, plus a "darker" text option that stroke-widens by 1 px | T1 | Compensates for low-contrast panels |
| Line height, margins, paragraph spacing vs first-line indent, alignment (justified, left), hyphenation (per-language patterns, no_std `hypher`) | T0/T1 | *(proven)* |
| Kerning, standard ligatures, small caps for headings | T1 | |
| Honour embedded CSS (toggle), publisher fonts (toggle) | T1 | |
| Anti-aliasing mode: 1-bit crisp vs 2-bit grey edges (uses panel 4-grey) | T2 | 4-grey costs an extra plane and ~130 ms |
| Focus reading (Bionic-style first-half bolding), guide dots | T2 | *(proven in forks)* |
| Typography profiles ("Novel", "Technical", "Night", user-named), switchable from the reader menu | T2 | |
| CJK: Noto Sans CJK subset packs on SD, vertical text later; ruby annotations | T2 | *(ruby proven)* Glyph packs stay on SD and are demand-paged |
| Right-to-left scripts (Arabic, Hebrew) | T3 | Requires shaping (`rustybuzz` no_std) and bidi; converter path first |

## 3. Reader chrome and status

| Feature | Tier | Notes |
|---|---|---|
| Footer styles: none, progress bar, chapter bar, percent, time left in chapter, time left in book, clock, battery; any combination, top or bottom | T1 | |
| Full-refresh cadence: every 1/3/5/10/15/30 pages, plus always after images and dialogs | T0 | *(proven)* |
| Manual refresh on a Power tap | T0 | |
| Reader quick menu (one screen, no nesting): TOC, bookmarks, go to, font size, brightness-of-text, orientation, dictionary, stats, sync | T0 | |
| Sleep screens: cover, poster (finish-by or streak), quote of the day, custom images (from SD or dropped via the phone, dithered on ingest, rotation fixed/each sleep/daily), quick resume, blank; a picker screen with live thumbnails and preview (07 screen 44) | T1 | *(proven)* |
| Key lock with a locked-state strip and hold-Power unlock; optional auto-lock on sleep (07 screen 45) | T1 | |
| Screenshot to SD (Power + Down) | T1 | |
| Position QR code for the phone | T2 | *(proven)* |

## 4. Library

| Feature | Tier | Notes |
|---|---|---|
| Library from SD with metadata and cover thumbnails cached in an index file (no rescanning on boot) | T0 | Cover thumbnail extraction happens once at import or on the converter |
| Views: Recent, All, Authors, Series, Collections (tags), Folders, Unread, Finished | T1 | |
| Sort by recent, title, author, added, progress | T1 | |
| Cover grid (2 × 3 on the 3.7" panel) and list view | T1 | |
| Search by title/author with the on-screen keyboard | T2 | |
| Delete, move to collection, mark finished/unread, reset progress | T1 | |
| Calibre metadata: read `metadata.opf` / `metadata.calibre` when present, honour series and tags | T2 | |
| Library management from the phone web UI (rename, collections, delete, covers) | T2 | |

## 5. Transfer, sync, network (design in 06)

| Feature | Tier | Notes |
|---|---|---|
| Wi-Fi station with saved networks (up to 8), captive-portal-free onboarding via a SoftAP set-up page | T0 | |
| **Drop page**: device hosts a web page at a `.local` name and a QR code; drag-and-drop or "Choose files" upload straight to SD | T0 | *(proven)* The iPhone/Android-first path |
| **Phone window**: the Drop page mirrors the device screen (1-bit PNG pushed over WebSocket), draws the seven keys as remote buttons, and provides a keyboard; every text field on the device offers "Type on your phone" | T1 | Signature (07); solves button typing |
| Streaming multipart and PUT uploads with progress on both ends, resumable | T1 | |
| WebDAV so the device mounts in iOS Files, macOS Finder, Windows Explorer, Android file managers | T1 | *(proven)* |
| OPDS catalog browser (Calibre-Web, Kavita, Komga and any user catalog), saved servers, search, download | T1 | *(proven)* |
| **Bookshop**: built-in free library with shelves from Project Gutenberg, Standard Ebooks and the Palace Bookshelf, offline searchable catalog on SD, Save for later, Get all (design in 09) | T1 | Project-hosted shelf index; downloads from the source hosts |
| Calibre wireless device connection | T1 | *(proven)* |
| Progress sync: KOReader sync protocol (self-hostable), plus our own JSON sync via WebDAV | T1 | *(proven)* |
| Send-to-device from iOS Shortcuts and Android Share (HTTP endpoint, see 06) | T1 | |
| Companion converter as a web app (runs in the browser, WASM) and a CLI: converts unsupported formats to `.qbk`, subsets fonts, prepares comics | T1 | Same Rust crates compiled to WASM |
| BLE: only for onboarding (send Wi-Fi credentials from the phone) and for a "nearby" beacon; not for book transfer | T2 | Too slow for files |
| OTA updates: from GitHub releases over Wi-Fi, from `update.bin` on SD, and a recovery partition triggered by a button combo | T0 | Mandatory because of USB-locked units |
| Time from NTP into the DS3231 RTC | T0 | |

## 6. Statistics and motivation

| Feature | Tier | Notes |
|---|---|---|
| Session log on SD (book, start, end, pages); per-page time capped at 120 s, ignored under 5 s | T1 | KOReader's method |
| **Analytics section** (07 screens 60a to 60e): Overview KPIs (time, pages, pages per hour, streak, books finished, time left in current book) with an ink-line bar chart per hour/day/month; Rhythm (time-of-day and weekday histograms, favourite hour and day, typical session, session list); Calendar heat map with streaks; Books table sortable by any column; Goals with quiet awards | T1 | All charts 1-bit, labelled directly |
| Per-book analytics block on Book info: that book's ink line, pace, and "Finish by" forecast from the last seven sessions | T1 | |
| Phone Analytics tab in the Drop page: interactive charts, CSV/JSON export, StoryGraph and Goodreads CSV, printable year poster | T1 | Rich version lives on the phone |
| Per-book: time, pages, average page time, time-to-finish book and chapter, started/finished dates | T1 | |
| Global: today, week, month, all time; pages per hour; time-of-day histogram; calendar heat map; year in review | T2 | |
| Streaks with a daily threshold and longest streak; yearly goal | T2 | |
| Awards page (quiet, never popups) | T3 | |
| Reading pet or plant, opt-in | T3 | *(proven, CrossPet)* |
| Export stats as JSON/CSV in the web UI; StoryGraph/Goodreads CSV | T2 | |

## 7. Apps

| App | Tier | Notes |
|---|---|---|
| Clock, calendar, stopwatch, pomodoro (RTC-backed, redraws once a minute) | T2 | |
| Flashcards with FSRS scheduling; decks as CSV/Anki-export text on SD; rate with the four bottom keys | T2 | |
| Read-later and news: RSS/Atom digest fetched on demand or on a schedule, paginated as a book; Hacker News front page and comments; Wikipedia | T2 | |
| Weather (Open-Meteo, no key) | T3 | |
| Calculator and unit converter | T3 | |
| Notes: short notes via keyboard, longer via the web UI; a "scratchpad" book | T3 | |
| File browser with a raw folder view | T1 | |
| Image viewer for photos on SD (dithered) | T1 | |
| Interactive fiction: Z-machine interpreter (v3/v5) with verb and noun pickers and command history | T3 | Proven on ESP32 class |

## 8. Games (all still-screen, button-first)

| Game | Tier | Notes |
|---|---|---|
| Sudoku (generator + pencil marks) | T2 | *(proven)* |
| 2048 | T2 | *(proven)* |
| Minesweeper | T2 | *(proven)* |
| Chess vs engine (3 levels) and two-player | T2 | *(proven)* |
| Checkers, Gomoku | T3 | |
| Wordle-style daily word | T3 | Offline word list |
| Solitaire (Klondike), Picross, crossword (.puz) | T3 | |
| Trivia packs, hangman | T3 | |
| Two-device play over ESP-NOW (chess, battleship) | T3 | |

## 9. System

| Feature | Tier | Notes |
|---|---|---|
| Runtime detection of UC8253 vs UC8279 panel and of X3 vs X4 board; one binary | T0 | |
| Power: light sleep between page turns, deep sleep after a timeout (1 to 60 min), wake on Power key, SD rail off in sleep, panel powered down when idle | T0 | |
| Battery gauge from the BQ27220 (percent, charging state, estimated days left) | T0 | |
| Settings screens for everything above; settings also editable in the web UI | T0/T1 | |
| Themes: a single "ink" theme with size variants; optional inverted (dark) mode | T2 | |
| Localised UI (English first; strings in one table, contributions welcome) | T1 | |
| Crash log to SD and a coredump partition; "report an issue" page in the web UI that bundles logs | T1 | |
| Developer menu: raw ADC readings for buttons, panel timings, heap graph, SPI speed test | T0 | Needed to verify ⚠ items in 02 |
| Recovery partition: reflash from SD by holding Back + Up at reset | T0 | |
| Plugin-ish extensibility: apps are compiled in but registered via a manifest, so contributors add a directory and a line | T1 | |

## Explicitly out of scope on this hardware

- Audio of any kind (no speaker, no audio path).
- USB mass storage (the ESP32-C3 has no USB OTG).
- Front light control (no front light).
- Handwriting or touch (no digitiser).
- Real-time video or animations (e-ink).
- DRM of any kind.
