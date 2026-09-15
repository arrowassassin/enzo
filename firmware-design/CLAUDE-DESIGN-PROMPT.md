# Start prompt for Claude Design

Paste the block below as the first message in Claude Design. It assumes Claude Design can read this repository on branch `claude/laughing-noether-mido1e`.

```
You are the UI designer for Quire, an open-source Rust e-reader firmware for the
Xteink X3 (a pocket e-ink reader).

Repository: arrowassassin/quire (GitHub)
Branch to read: claude/laughing-noether-mido1e   <- not main; main has unrelated code
Folder: firmware-design/

Read these before designing anything, in this order:
1. firmware-design/07-ux-design-brief.md  — the brief. Section 0 is the rulebook;
   sections 1 to 11 are the reference. This file is the source of truth.
2. firmware-design/02-hardware.md, sections 2 and 5 — the panel and the keys.
3. firmware-design/09-bookshop.md, section 4 — the Bookshop screens.
4. firmware-design/05-feature-catalog.md — so nothing is designed that the
   firmware will not have, and nothing the firmware has is left out.

Non-negotiables (from the brief):
- Every screen is a static page on a 528 × 792 px portrait canvas, pure 1-bit
  black on white. No greys, gradients, shadows, transparency, animation or
  scrolling. Hatching and dot screens stand in for grey; inversion is focus.
- Input is seven keys only: Left, Back, Confirm, Right along the bottom edge;
  Up, Down on the right edge; Power on top. No touch. Every screen shows the
  edge-label rail (40 px, bottom) and side labels where the side keys act.
- The six signature elements must appear wherever the brief says they do:
  the book is the home screen; edge labels and the compass; the Spine;
  time not percent; phone as window and keyboard; Jump.
- Two-press rule: every common reading action within two presses from the page.
- Type: Literata for titles and reading text, Atkinson Hyperlegible for labels
  and lists, JetBrains Mono for edge labels, times and numbers. Scale in
  brief section 4. Minimum sizes in section 0.
- Square corners. Three line weights only: 4 px heavy rule, 2 px rule,
  1 px hairline (Spine and inert dividers).
- Use real content: the sample text and titles in the brief, real public
  domain books, real numbers. Never lorem ipsum.

Produce, in this order, and stop after each batch for review:
Batch 1 — the foundation
  a. Typography board (brief section 4) at 1:1.
  b. Components board (brief section 5), every component with focused and
     disabled variants.
  c. Signature board: the six elements isolated, the Spine in three states
     (reading, skim, contents), a poster-numeral group, the phone window.
Batch 2 — the reading core
  20 Reading page (all the variants listed), 21 Compass (both pages),
  10 Home layer, 22 Contents, 23 Go to, 24 Skim (three frames),
  25 Type and Layout, 26 Word cursor, 27 Dictionary, 40 Sleep screens
  (all variants), 44 Sleep screen picker, 45 Keys locked.
Batch 3 — getting books
  30 Drop page (all variants), 31 Wi-Fi, 35 to 39 Bookshop, 43 Phone window.
Batch 4 — library, analytics, system
  11 Library, 12 Book info, 2A End of book, 42 Jump, 50 Settings (Keys,
  Sleep and power, About), 51 OTA, 60a to 60e Analytics, 61 Year in review,
  plus a charts board (ink line, histogram, heat map, goal Spine) at 1:1.
Batch 5 — apps, games, first run, edge cases
  01, 02, 70 to 76, 80 (the five named games), 90, 99.

For every artboard: name it NN-screen-state as in the brief, mark whether
entering it is a partial (DU) or full (GC) refresh, and include the states the
brief requires (default, focused, empty, working, error; dark mode where listed).
Export each at 1:1 and also as a 1-bit PNG so dithering can be checked.

Before showing each batch, check your own work against brief section 0 and
list any screen where the two-press rule or the key grammar did not hold, with
your proposed fix. If the brief is silent on something, choose the option that
looks most like a well-typeset book and say what you chose.
```

Follow-up prompts that work well after each batch:

- "Show batch 2 screen 20 at 22 px and 34 px side by side with the Spine; I want to judge the text block."
- "Redo the compass with the labels 30% larger; they must be readable at arm's length."
- "Give me the refresh map for everything so far as one page."

## Add-on prompts for the newer elements

Paste each as its own message after the start prompt, or after the batch it belongs to.

### Analytics section (60a to 60e)

```
Design the Analytics section from brief section 6, screens 60a to 60e, plus the
charts board. These are 1-bit e-ink charts with no colour, no legend, no grid
lines heavier than a 1 px hairline, and every mark labelled directly.

Chart vocabulary (design these on the charts board first, at 1:1):
- Ink line: a bar chart, one solid black bar per unit (hour, day, or month),
  4 px minimum bar width, 2 px gaps, baseline as a 2 px rule, the tallest bar
  carrying its value in JetBrains Mono 18 px above it, the axis labelled at the
  first, middle and last position only.
- Histogram: same as the ink line but 24 columns (hours) or 7 (weekdays), the
  favourite column inverted with its label beneath in small caps.
- Heat map: a 7-column calendar, cells 56 × 56 px with 4 px gaps, fill levels
  0 / 25 / 50 / 100 % of the daily goal drawn as empty / sparse dots / dense
  dots / solid; today gets a 3 px outline; consecutive streak days are joined by
  a 2 px rule through their centres; weekday initials in small caps above.
- Goal Spine: the yearly goal as a vertical hairline strip like the reading
  Spine, filling solid as books finish, with the count as a poster numeral.
- Poster numeral: Literata 44 px numeral with an 18 px small-cap label under
  it; tiles in a 2-column grid separated by 2 px rules, never boxed.

Screens:
60a Overview — tab line Today · Week · Month · Year · All (Left/Right), six
  poster numerals in a 2 × 3 grid (`1 h 42` READ, `86` PAGES, `31` PAGES/HOUR,
  `12` STREAK DAYS, `2` BOOKS FINISHED, `4 h 10` LEFT IN CURRENT BOOK), then the
  period's ink line. Rail: `Back · — · Books · Goals`; side labels Rhythm /
  Calendar.
60b Rhythm — 24-column time-of-day histogram, 7-column weekday chart, three
  poster numerals (`9 pm` FAVOURITE HOUR, `Sun` FAVOURITE DAY, `27 min` TYPICAL
  SESSION), then a session list for the selected day in mono (start, length,
  book), paginated.
60c Calendar — the heat map for one month, poster numerals `12` CURRENT
  STREAK · `41` LONGEST · `19 / 30` DAYS THIS MONTH; Left/Right change month.
60d Books — a table: title, time, pages, pages/hour, started, finished; the
  focused column head is inverted; long-Confirm on a head sorts by it; a row
  opens Book info. Show the table at 8 rows per page.
60e Goals — daily goal stepper (minutes or pages), yearly goal stepper (books),
  the Goal Spine, and an awards list rendered as small-cap lines with a date
  (first book, 7-day streak, 100 hours, night owl). No badges, no popups.

States: default, focused, empty ("No reading yet today"), and dark mode for
60a. Use real numbers that agree with each other across screens.
```

### Bookshop (35 to 39)

```
Design the Bookshop, brief screens 35 to 39 and 09-bookshop.md section 4. It is
a small, opinionated bookshop, not a search engine: shelves first, search second.

35 Bookshop home — six shelves, each a row of three covers (152 × 228, 2 px
  frames) plus a fourth "More" cell drawn as a hatched frame with the word
  More; shelves: Start here · Popular this week · New editions · Modern &
  Creative Commons · Collections · By subject. Two shelves fit per page;
  Up/Down move between shelves and page. A Search row at the top with the
  phone hint ("Type on your phone" + a 64 px QR). Language filter in the
  running head. First-visit line under the title: "Free, open books. No
  account, no DRM."
36 Book page — cover left, title 32 px Literata, author, year · language ·
  source line ("Standard Ebooks · public domain"), poster numeral `6 h` TO
  READ, the blurb paginated at 22 px. Rail: `Back · Save · Get · More`. States:
  not downloaded, downloading (Get becomes a stepped bar with KB/s in mono), in
  library (Get reads Read), error ("Gutenberg is limiting requests, retrying
  in 30 s"), and Wi-Fi off ("Connect to get this book" with a Wi-Fi action).
37 Search — results as you type, from the offline catalog, sorted by
  popularity; each row: title, author, year, a small source glyph (G / SE /
  P); the keyboard beneath with the phone strip above it; works with Wi-Fi
  off.
38 Browse — Subjects, Authors A to Z (letter rail on the right using Up/Down),
  Collections with one-line intros, Languages.
39 Downloads and Saved — queue rows with a stepped bar and a mono status; the
  Saved list with a `Get all` action in the rail.

Use real public-domain titles and authors throughout.
```

### Sleep screen picker and key lock (44, 45)

```
Design 44 Sleep screen picker and 45 Keys locked from brief section 6.

44 — a 3 × 2 grid of live thumbnails at 1:4 (132 × 198) rendered from the
  current book: Cover, Poster, Quote, Custom, Quick resume, Blank; 6 px frame
  on focus, a small "In use" check under the active one. Beneath the grid, the
  options for the focused variant as toggle or choice rows (Cover: title band
  on/off; Poster: Finish by or Streak; Quote: built-in or quotes.txt; Custom:
  folder, rotation fixed / each sleep / daily; Quick resume: moon glyph
  on/off). Rail: `Back · — · Use · Preview`. Also design the Preview state
  (the full-size sleep screen with a 3 s countdown in the corner).
45 — the reading page with a lock glyph at the right of the running head;
  the one-refresh strip at the top: "Keys locked · hold Power to unlock", 48
  px tall, inverted; and the same lock glyph placed on the Cover sleep screen's
  band.
```

### Phone window and "Type on your phone" (43 and the keyboard strip)

```
Design 43 Phone window as it appears inside the Drop page on a phone (390 ×
844 frame), in the same 1-bit visual system so it feels like one product:
the device screen mirrored at 1:2 (264 × 396) inside a 2 px frame, the seven
keys drawn around it in their physical positions as tappable 44 px targets
(four under the bottom edge, two on the right edge, Power above), a text field
beneath with a "Send" button, and the phone's own keyboard area. Show: idle,
a search being typed (the mirrored device shows the same text in its field),
and "Sent to reader" feedback. Also design the device-side "Type on your
phone" strip that sits above every on-device keyboard: 56 px tall, a 48 px
QR at the left, the words "Type on your phone" and the URL in mono.
```

### Settings additions from the review (Keys, Battery, Night jobs)

```
Design three settings pages from brief section 6, screen 50:
- Keys: a line drawing of the device (2 px strokes) with each of the seven
  keys labelled by its current function and a Remap action; rows for side-key
  swap, orientation follow, tilt-to-turn, shake-to-turn, tap-to-turn (with a
  sensitivity stepper), and a Remote row that opens a pairing page for a BLE
  page-turner or keyboard (scanning state as a Working card, paired state as
  a row with the device name and a Forget action).
- Battery: poster numerals `19 days` LEFT · `62 %` CHARGE · `41` CYCLES ·
  `Good` HEALTH, then a 30-day ink line of daily consumption, then the
  charging state line ("Charging · 0.42 A") and a note on estimate basis.
- Night jobs: a master toggle, an hour stepper (`6:00`), toggles for News,
  Bookshop shelves, Sync, Catalog refresh, and a "Last run" line with the
  result ("Today 06:02 · 3 articles, shelves updated").
```

## Batch 1 acceptance check (paste before Batch 2)

```
Before Batch 2, audit Batch 1 - Foundation against firmware-design/07-ux-design-brief.md
section 0, 3, 4 and 5, and report a pass/fail line per item. Fix every fail
in place, then show the corrected boards.

Typography board
- Exactly three faces: Literata (titles, reading text, poster numerals),
  Atkinson Hyperlegible (labels, lists), JetBrains Mono (edge labels, times,
  table numbers). Nothing else, no Inter, no system fallback showing.
- UI scale present and labelled at 1:1: 18 label/small caps, 22 body,
  26 list title, 32 screen title, 44 poster numeral, 56 hero numeral.
- Reading scale present: 20 22 24 26 28 31 34 38 px at line heights
  1.3 / 1.45 / 1.6, with the 26 px / 1.45 / 32 px-margin default marked.
- A chapter opening specimen: 56 px numeral, 32 px title, 4 px rule, three-line
  drop cap.

Components board (each with default, focused, disabled)
- Running head (36 px, 4 px rule under the title on non-reading screens).
- Edge-label rail: 40 px, four equal cells, 4 px rule above, JetBrains Mono
  18 px, empty cells show a centred 2 px dot.
- Side labels rotated 90° on the right edge.
- Compass overlay: card over the lower 40 %, text above screened with a 50 %
  dot pattern, choices placed at the key positions, 26 px label + 18 px context.
- Home layer: card over the upper 60 %, title, author, "38 min left in this
  chapter", "Finish by Thursday", three recent rows, the rail
  Library · Close · Continue · Bookshop.
- Jump list, list row 56/88 px, cover grid cell 152 × 228 with 6 px focus
  frame, poster numeral, toggle/stepper/choice/slider rows, dialog card,
  working card (no spinner), empty state, keyboard 48 × 56 keys with the
  "Type on your phone" strip above it, T9 variant, specimen, phone window.
- The Spine: 12 px strip, 1 px hairlines at 4 px pitch, read = full width,
  unread = 6 px, 3 px chapter notches, 2 px position bar; three states
  (reading, skim with the riding label, contents).

Global rules
- Pure #000 on #FFF only; any grey, gradient, shadow, transparency or
  rounded corner is a fail. Dither only inside images.
- Focus is full inversion of the row/cell/button, never an outline alone.
- Strokes: 4 / 2 / 1 px only; no icon set beyond the 24 named in section 3.
- Sizes: body ≥ 22 px, captions ≥ 18 px, rows ≥ 56 px, rail 40 px.
- Every artboard names its refresh (DU or GC) and is on a 528 × 792 frame
  with 24 px margins (components board excepted).
- Copy is sentence case, numerals, no exclamation marks, no emoji.
```

## Batch 2 kickoff (paste after the audit passes)

```
Batch 2 — the reading core. Build only from the approved Batch 1 boards; do
not restyle a component here, change it on the components board and reuse it.
Screens and states per brief section 6:

20 Reading page — at 22, 26 and 34 px, line heights 1.3 and 1.6; variants:
   chapter opening with numeral and drop cap, inline image (dithered),
   footnote marker, verse, code block, stacked table, last page, landscape.
   The Spine on every one. No rail on the reading page. Mark: GC on entry,
   DU per turn.
21 Compass — page 1: Left Contents · Right Go to · Up Type · Down More ·
   Back Close · Confirm Bookmark (and "Bookmarked ✓"); page 2 (More):
   Left Dictionary · Right Highlights · Up Layout · Down Sleep · Confirm
   Stats. GC on open and close.
10 Home layer — over the page, plus the no-book variant with three Start
   here picks and the Drop QR.
22 Contents — nested TOC, current chapter with a filled bookmark, time-to-read
   per chapter in mono on the right, the Spine drawn alongside.
23 Go to — 44 px percent numeral with Left/Right stepping, chapter picker,
   "Page 123 of 412", the skim hint.
24 Skim — three frames of a held Right: the Spine position bar moving with
   the riding label "Ch 14 · 51% · 2 h 03 left".
25 Type and Layout — two screens; Type with the live specimen occupying the
   top half; Layout with every row named in the brief including drop caps,
   running head, Spine on/off, full-refresh cadence, profiles.
26 Word cursor — cursor on the rarest word with the mono 1/6 counter; a
   selection spanning two lines.
27 Dictionary — headword 32, pronunciation 22, paginated definitions, rail
   Back · Save word · Wikipedia · Next dict.
40 Sleep screens — Cover, Poster, Quote, Quick resume, Custom, Charging,
   Empty battery.
44 Sleep screen picker and 45 Keys locked as specified.

Deliver dark-mode variants for 10, 20, 21 and 40. Use the sample text from
the brief's mock (The Left Hand of Darkness opening) and real public-domain
titles elsewhere. Before showing the batch, list any screen where the
two-press rule or the key grammar did not hold, with your proposed fix.
```
