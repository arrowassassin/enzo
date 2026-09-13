# 07 — UX design brief (hand-off for Claude Design)

This document is written to be pasted into Claude Design as the project instructions. Section 0 is the short version to paste first; the rest is the reference the designer (human or Claude) works from screen by screen.

---

## 0. Paste-in instructions for Claude Design

You are designing the user interface of an open-source e-reader firmware for the Xteink X3, a pocket e-ink reader. Design every screen as a **static, print-like page** on a **528 × 792 px portrait canvas** (the panel is 3.68 inches at 259 PPI, so 1 px ≈ 0.1 mm; treat 10 px ≈ 1 mm). The device has **no touch screen**. Input is seven physical keys: four along the bottom edge (Left, Back, Confirm, Right), two on the right edge (Up, Down), and a Power key on top. Every screen must be fully operable with those keys and must make the current focus unmistakable.

Hard rules:
1. **Black and white only.** Design in pure 1-bit: black ink on white paper. No greys, no gradients, no shadows, no transparency. Use hatching or dot patterns where you would normally use grey (disabled items, secondary fills). One exception: images (book covers, photos) may be shown as dithered greyscale.
2. **No animation, no motion, no scrolling.** Every state is a separate static page. Lists paginate. Progress is a stepped bar redrawn at most once a second.
3. **Focus is inversion.** The focused list row, button, or cell is drawn white-on-black across its full width. Never a thin outline as the only focus cue.
4. **Minimum sizes.** Body text 22 px or larger (10.5 pt at 259 PPI), captions no smaller than 18 px, UI strokes 2 px, focus ring 3 px, icons at 24 or 32 px on a 2 px stroke grid, list rows at least 56 px tall.
5. **Typography.** UI text: a humanist sans (Inter, Atkinson Hyperlegible, or Noto Sans). Book text: a screen serif (Literata, Bookerly-class). Sizes from the scale in section 4. Left-align UI text. Use sentence case. Never all-caps for more than 3 words.
6. **One purpose per screen.** No badges, no notification toasts, no nested modals. A dialog is a full-width card with at most two actions, and it maps its actions to Back (cancel) and Confirm (accept).
7. **Every screen carries a key hint bar** at the bottom: up to four short labels aligned over the four bottom keys (e.g. `◀ Prev · Back · Open · Next ▶`), plus an Up/Down glyph at the right edge when the side keys do something. The hint bar is 40 px tall.
8. **Status strip** at the top of non-reading screens: screen title on the left, clock and battery glyph on the right, 44 px tall, 2 px rule under it.
9. **Reading screen is sacred.** Only the text and a single optional one-line footer. Nothing else appears unless the user presses a key.
10. **Design the states.** For each screen deliver: default, focused item, empty state, loading (a static "Working…" card, no spinner), error, and the same screen in inverted "dark" mode (white on black) where relevant.

Deliverables: one artboard per screen and state, named `NN-screen-state`, in the order of the screen inventory in section 6. Use a 528 × 792 frame, 8 px grid, 24 px outer margins. Include a components board with the library in section 5 and a typography board with the scale in section 4.

---

## 1. Who this is for and how it should feel

The owner is a reader who bought a $79, 58-gram device because it fits in a jeans pocket and lasts weeks. They are technical enough to flash firmware and want the device to be *fast, quiet, and thorough*: everything the big readers do, nothing that gets in the way of the page. The feeling to aim for is a well-made paperback with a great index, not a tablet. Delight comes from speed, from typography, and from quiet touches (a cover on the sleep screen, "38 min left in this chapter"), never from visual noise.

Design values, in priority order: legibility → speed (fewest key presses, fewest refreshes) → predictability (same keys do the same thing everywhere) → completeness → charm.

## 2. Physical model

**Canvas.** 528 wide × 792 tall in portrait. The firmware also supports all four orientations; design portrait first and landscape (792 × 528) only for the reading screen and image viewer.

**Keys and their universal meaning.**

| Key | Everywhere | In the reader |
|---|---|---|
| Left (bottom, outer left) | Move focus left, or previous page of a list | Previous page |
| Back (bottom, inner left) | Go back / cancel; long-press: Home | Short: toggle footer; long: Home |
| Confirm (bottom, inner right) | Activate focused item; long-press: context actions | Short: reader menu; long: word cursor (dictionary, highlight) |
| Right (bottom, outer right) | Move focus right, or next page of a list | Next page; hold: skim |
| Up / Down (right edge) | Move focus up/down; hold: fast repeat | Previous / next page (user can remap to chapter) |
| Power (top) | Short: sleep (or configurable: refresh); long 2 s: power menu | Same |

Long-press is 500 ms. Double-press is not used anywhere except the developer menu, to keep the model simple. Hold-repeat starts after 500 ms at 5 per second.

**Refresh.** A key press must change the screen within 100 ms of firmware time plus one partial refresh (about 400 ms). Full refresh (a black-white flash, ~500 ms) happens: every N pages in the reader (default 10), when entering or leaving the reader, when a dialog opens or closes, and after any screen with an image. The designer should mark on each artboard whether entering it is a partial or a full refresh.

**Sleep.** After the idle timeout the device shows the sleep screen and powers the panel down. The sleep screen is the only screen that stays visible for hours, so it deserves the most craft.

## 3. Visual language

- **Ink and paper.** Pure black (#000) on pure white (#FFF). Inverted mode swaps them globally.
- **Patterns instead of grey.** Disabled: 50% checkerboard dots. Secondary surface (e.g. a card background): 2 px horizontal hatch at 8 px pitch. Selected-but-not-focused: 3 px left bar. Focused: full inversion.
- **Rules and boxes.** 2 px lines. Cards have a 2 px border and square corners (rounded corners dither badly at this resolution; a 4 px radius is the maximum).
- **Icons.** Simple 2 px stroke line icons on a 24 px grid, filled variants for the active state. Keep the set small (about 40 icons): book, library, folder, search, settings, wifi, wifi-off, battery ×5, charging, clock, bookmark, bookmark-filled, dictionary, highlight, note, sync, download, upload, check, close, chevron ×4, arrow ×4, refresh, sleep-moon, sun, font, text-size, spacing, margins, align ×3, orientation, stats, streak-flame, calendar, game, apps, info, warning, qr, lock, trash, plus, minus, more.
- **Covers.** Dithered greyscale (Floyd–Steinberg) inside a 2 px frame. Grid cells 152 × 228 px (2:3). If no cover, generate a typographic cover: title in the serif at 26 px, author at 18 px, on a hatched background, all inside the same frame.
- **Progress.** A 6 px tall bar with a 2 px border, filled solid; chapter ticks as 2 px marks above the bar.
- **QR codes.** Version 3 to 6, module size at least 6 px, quiet zone 4 modules; always with the human-readable text beneath.

## 4. Typography scale

UI font (sans): 18 caption · 22 body · 26 list-title · 32 screen-title · 44 display (numbers on stats and sleep screens). Line height 1.3. Weights: regular and bold only.

Book font (serif, user-adjustable): steps 20 · 22 · 24 · 26 · 28 · 31 · 34 · 38 px, default 26 px. Line height user-selectable 1.3 / 1.45 / 1.6, default 1.45. Margins 16 to 48 px in 8 px steps, default 32 px. Justified with hyphenation by default. At 26 px with 32 px margins, a line holds about 38 characters and a page about 24 lines, which is the paperback feel we want.

Hierarchy is made with size, weight, and whitespace, never with colour.

## 5. Component library

Deliver each on the components board, with focused and disabled variants.

1. **Status strip** (44 px): title left, right-aligned cluster of Wi-Fi glyph (only when on), clock `14:32`, battery glyph with percent optional.
2. **Key hint bar** (40 px): four equal cells over the four bottom keys, 18 px labels, a thin 2 px rule above; a fifth narrow cell at the right edge shows `▲▼` when the side keys act.
3. **List row** (56 px, or 88 px two-line with a thumbnail): title 26 px, subtitle 18 px, optional right-aligned value or chevron. Focus inverts the whole row.
4. **Cover grid cell** (152 × 228 cover + 2 lines of text): 3 columns × 2 rows per page with 24 px margins and 16 px gutters. Focus draws a 6 px black frame around the cell.
5. **Section header**: 18 px bold caps-free label with a 2 px rule.
6. **Toggle row**: label left, a 44 × 24 px switch glyph right (filled square when on).
7. **Stepper row**: label left, `−  26 px  +` right; Left/Right change the value when the row is focused.
8. **Choice row**: label left, current value right; Confirm opens a full-page picker list.
9. **Slider** (for go-to-percent): a 12 px tall track with a 24 px knob; Left/Right step 1%, hold accelerates.
10. **Dialog card**: full-width card with 2 px border, 32 px title, 22 px body, two actions laid on Back/Confirm. Always drawn with a full refresh.
11. **Progress card**: title, a stepped bar, a one-line status. Used for transfers, ingest, OTA.
12. **Empty state**: a 64 px icon, one line of 26 px text, one line of 22 px hint, and the key hint that fixes it.
13. **Keyboard**: 3-row QWERTY plus a numbers row, each key 48 × 56 px, focus inversion, Left/Right/Up/Down move, Confirm types, Back deletes, long-Confirm toggles case/symbols, the text field above shows a 2 px caret. Also a compact **T9-style** 3 × 4 grid variant for search.
14. **Footer (reader)**: single line, 18 px sans, options: progress bar, `43%`, `Ch 12 of 31`, `38 min left in chapter`, `2 h 10 min left`, clock, battery.
15. **QR pair**: two QR codes side by side with labels "Join Wi-Fi" and "Open page", and the URL in 22 px mono beneath.
16. **Stat tile**: 44 px number, 18 px label, 2 px frame; tiles in a 2-column grid.
17. **Calendar heat map**: 7 columns, each day a 20 px square filled 0/25/50/100% with dot patterns.
18. **Toast is not a component.** Feedback is shown inline (a checkmark next to the item) or as a dialog.

## 6. Screen inventory

Order is the design order. Each entry lists content and key behaviour. States required for all: default, focused, empty, error where applicable, and dark mode for 01, 10, 11, 40.

**01 Boot** — Wordmark, version, a stepped bar for "Indexing library". Full refresh only, shown for at most 2 s.

**02 First-run welcome** — Language choice, then "Set the time", then "Connect Wi-Fi (optional)", then "Add books" offering two paths: 30 Drop page and 35 Bookshop. Each is a single screen with one focused primary action.

**10 Home** — The most-used non-reading screen. Top: the current book as a wide card (cover left 96 × 144, title, author, `43% · 38 min left in chapter`, focused by default so a single Confirm resumes reading). Below: a 2-column grid of the five other recent books as small covers. Bottom: a row of five icon buttons: Library, Bookshop, Drop (transfer), Apps, Settings. Key hints: `Library · — · Continue · Drop`. Long-Confirm on a book: actions (Open, Book info, Mark finished, Remove).

**11 Library** — Tabs across the top (Recent · All · Authors · Series · Collections · Folders), switched with Left/Right when the tab bar is focused. Body: cover grid (3 × 2) or list (8 rows), toggled from the menu. Page indicator `3 / 12` at the right of the tab bar. Up/Down move focus, then page. Long-Confirm on a book opens the actions sheet. Sort and view options live in a small menu on Back-long. Include a "Search" entry that opens the keyboard.

**12 Book info** — Cover large left, title, author, series with index, tags, format, size, added date, progress, reading time so far, estimated time to finish. Actions: Open, Mark finished/unread, Add to collection, Delete, Reset progress.

**13 Folders** — Raw SD browser: name, type icon, size; breadcrumb line; Back goes up a level.

**20 Reader** — Text only, with the optional single-line footer top or bottom. Design the page at three font sizes and both line-height extremes. Show: a chapter opening page (chapter title styling), a page with an inline image, a page with a footnote marker, a poem (left aligned, preserved lines), a code block (mono), a table rendered as stacked rows, and the last page of a book.

**21 Reader menu** — Opens on Confirm as a full-width card over the lower half of the page (the text stays visible above, dimmed with a dot pattern). Two rows of four icon buttons: Contents, Bookmarks, Go to, Search / Font, Layout, Orientation, More. Left/Right/Up/Down move, Confirm opens, Back closes. Show the "More" page: Dictionary, Highlights, Book info, Stats, Sync now, Auto-turn, Sleep.

**22 Contents** — Nested TOC with indentation by 24 px per level, current chapter marked with a filled bookmark glyph, page indicator; Confirm jumps.

**23 Go to** — Percent slider with a large `43%` readout, chapter picker beneath, and a "Page 123 of 412" stepper. Also shows the "Skim" hint: hold Right in the reader.

**24 Skim overlay** — While Right is held: the page underneath keeps turning, and a 64 px tall strip at the bottom shows a progress bar with chapter ticks and `Ch 14 · 51%`. Release lands. Design the strip.

**25 Font & layout** — Two screens. Font: family list with each name rendered in its own face, size stepper with a live sample paragraph, weight/darker toggle, anti-alias toggle. Layout: line height, margins, alignment, hyphenation, paragraph style, embedded styles toggle, publisher fonts toggle, footer options, full-refresh cadence, profiles (save / apply).

**26 Word cursor** — Long-Confirm in the reader: a 2 px underline cursor on the first word of the page; Left/Right/Up/Down move it word by word and line by line; Confirm opens 27 Dictionary; long-Confirm starts a selection that extends with Right; Back exits. Show a selection spanning two lines.

**27 Dictionary** — Headword 32 px, pronunciation 22 px, definitions as a paginated body, "Save word" and "Wikipedia" actions in the hint bar. Multiple dictionaries switch with Left/Right on the header.

**28 Highlights & notes** — List grouped by chapter: the highlighted sentence in serif 22 px, note beneath in sans 18 px. Actions: Open, Add note (keyboard), Delete, Export.

**29 Footnote popup** — A card over the lower third with the note text, paginated if long, `Back to text` hint.

**2A End of book** — "You finished *Title*": time taken, pages, started/finished dates, Rate (5 focusable stars), Mark finished, Next in series (with cover), Back to library.

**30 Drop page (on device)** — Big title "Drop books here", the QR pair, the URL and IP in 26 px, then a live list of arriving files with per-file stepped bars and states, and a footer line "Wi-Fi: HomeNet · 3 files added". Variants: hotspot mode (shows the Wi-Fi name and password in text), not connected (offers Set up Wi-Fi), transfer complete, error (disk full).

**31 Wi-Fi setup** — Saved networks list with signal bars, "Add network" opens a scan list, then a password keyboard. Connection status card with IP and `.local` name.

**32 OPDS catalogs** — Saved servers list, then a catalog browser: entries with title, author, a download glyph; search via keyboard; downloading shows a progress card.

**33 Calibre connect** — A single status screen: "Waiting for Calibre… listening on 192.168.1.20:9090" with a stepped activity indicator; connected state shows the transfer list.

**34 Sync** — Position sync status per book, last sync time, "Sync now", server settings.

**35 Bookshop home** — Six shelves, each a row of three covers plus a "More" cell: Start here, Popular this week, New editions, Modern & Creative Commons, Collections, By subject. Search entry at the top, language filter in the status strip. Up/Down move between shelves, Left/Right within a shelf. A one-line note on first visit: "Free, open books. No account, no DRM."

**36 Book page (Bookshop)** — Cover left, title 32 px serif, author, year, language, "about 6 h" reading estimate, source and licence line, blurb paginated. Primary action **Get** (turns into a stepped progress bar, then **Read**); secondary: Save for later, More by this author, Same collection. Design the states: not downloaded, downloading, in library, error ("Gutenberg is limiting requests, retrying in 30 s").

**37 Bookshop search** — Keyboard at the bottom, live results above from the offline catalog (title and author prefix), sorted by popularity, each row with a source glyph. Works with Wi-Fi off; show the "Connect to get this book" state.

**38 Bookshop browse** — Subjects, Authors A to Z, Collections (with one-line descriptions), Languages. A collection page is a list with covers and a short intro.

**39 Downloads & Saved** — Download queue with per-item progress and errors; Save-for-later list with a **Get all** action.

**40 Sleep screens** — Design five: Cover (full-bleed dithered cover with a bottom band: title and `43% · 38 min left`), Cover + streak, Custom image, Quote of the day (serif quote, attribution, date), and Quick resume (the last page at 50% dots with a moon glyph and "Press Power to wake"). Also the "Charging" variant with a large battery percent, and the "Battery empty" screen.

**41 Power menu** — Sleep, Power off, Restart, Refresh screen, Toggle Wi-Fi, Lock keys. Long-press Power opens it.

**50 Settings** — Top-level groups as a list: Reading, Display, Buttons, Sleep & power, Wi-Fi & sync, Library, Language & time, Apps & games, About, Developer. Each group is a list of toggle/stepper/choice rows, paginated. Design Buttons (a diagram of the device with each key labelled and remappable, plus side-key swap and orientation follow), Sleep & power (idle timeout, sleep screen choice, panel-off, battery view with days-left estimate), and About (version, storage bars, "Check for update", licences).

**51 OTA update** — Available version card with release notes paginated, "Install" action, progress card, "Restart" dialog. And the SD-update and recovery variants.

**60 Stats** — Today / This week / All time tabs. Tiles: time, pages, pages per hour, books finished. Then: time-of-day histogram (24 bars), calendar heat map (this month), streak card with flame glyph and "Longest 41 days", yearly goal bar. Per-book stats reachable from Book info.

**61 Year in review** — A poster-style page: big numbers, top 5 books as small covers, favourite hour, longest session. Shareable as a screenshot.

**70 Apps** — Grid of 3 × 3 icon tiles with labels: Clock, Flashcards, News, Wikipedia, Weather, Calculator, Notes, Images, Interactive fiction. Second page: Games.

**71 Flashcards** — Deck list; card front (serif, centred, large); card back; rating row over the four bottom keys `Again · Hard · Good · Easy`; session summary.

**72 News / read-later** — Feed list with unread counts, article list (title, source, time, read state), article view uses the reader screen.

**73 Clock & timer** — A big clock with date, a pomodoro with a 25/5 ring drawn as a 2 px-stepped circle, alarm list (visual-only wake).

**74 Weather** — Today card with a 64 px icon, 44 px temperature, 5-day row.

**75 Image viewer** — Full-bleed dithered image, filename strip, Left/Right to browse, Confirm to toggle fit/1:1.

**76 Interactive fiction** — Transcript paginated as a book, a command line at the bottom, and pickers: a verb row and a noun grid so common commands need no typing.

**80 Games hub** — Grid: Sudoku, 2048, Minesweeper, Chess, Checkers, Gomoku, Wordle-style, Solitaire, Picross, Trivia. Each game screen: board sized to the 480 px inner width, cursor as inversion of the cell, a compact status line, and a paused/menu card. Design Sudoku (9 × 9 at 52 px cells, pencil marks at 14 px), 2048 (4 × 4 at 112 px), Minesweeper (16 × 16 at 30 px), Chess (8 × 8 at 60 px with clear piece glyphs, hatched dark squares), and Wordle (5 × 6 tiles at 72 px plus a compact keyboard with state marks).

**90 Developer** — Raw ADC readings for keys, panel type and timings, heap free graph (stepped), SPI benchmark, log tail. Mono font throughout.

**99 Recovery** — Minimal: "Recovery mode", options Reflash from SD, Restart, with the version and a hint. Deliberately ugly-proof: 32 px text, no icons.

## 7. Flows to storyboard

1. **First run to first page**: 01 → 02 (4 steps) → 30 → upload from phone → 10 → 20. Target: under 3 minutes, and the user never types on the device except the Wi-Fi password.
2. **Resume reading from sleep**: Power → 40 (quick resume) → 20 in one full refresh.
3. **Look up a word**: 20 → long-Confirm → 26 → Confirm → 27 → Back → 20. Four presses.
4. **Change font size**: 20 → Confirm → 21 → Font → 25 (live sample) → Back → 20. The page re-flows on return with a full refresh.
5. **Add a book from an iPhone**: 10 → Drop → 30; phone: scan, Safari, choose file, done → 30 shows "Ready" → Confirm opens the new book.
6. **Finish a book**: last page → Right → 2A → Next in series → 20.
7. **Sudoku from Home**: 10 → Apps → 70 → Games → 80 → Sudoku, 4 presses; the game remembers its state across sleep.
8. **Get a free book with no computer**: 10 → Bookshop → 35 → Popular → 36 → Get → Read. Five presses to a new book. Also storyboard the offline variant: search with Wi-Fi off, save three books, connect later, Get all from 39.

## 8. Copy and tone

Short, plain, warm, never cute. "Drop books here", "38 min left in this chapter", "Nothing here yet. Send a book from your phone: press Drop." Use numerals. Never blame the user in an error: "Couldn't join HomeNet. Check the password." Titles in sentence case. No exclamation marks. No emoji in the firmware UI (the icon set covers it).

## 9. Accessibility and comfort

- A "Large UI" setting scales the UI type scale by 1.25 and list rows to 72 px.
- Inverted mode for the whole UI and the reader.
- Left-handed: rotate 180° and the side keys sit under the left thumb; the footer and hint bar follow the rotation.
- A "Simple mode" hides Apps and Games from Home.
- Key lock (Power menu) to avoid page turns in a pocket.

## 10. What to send back

- Artboards named as in section 6, exported as PNG at 1:1 (528 × 792) and also as 1-bit PNG to check dithering.
- The components board and the typography board.
- A one-page "refresh map": which transitions are partial and which are full.
- Notes on any screen where the key model felt awkward, with a proposed alternative.
