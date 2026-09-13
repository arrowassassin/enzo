# 07 — UX design brief (hand-off for Claude Design)

Second edition, after a design-lead review. The first edition was correct and forgettable: a competent Kindle on a smaller screen. This edition gives the product a thesis and six signature elements that no shipping e-reader has, a two-press rule for speed, and a performance budget that the design must respect. Section 0 is the block to paste into Claude Design. Everything after it is the reference.

---

## 0. Paste-in instructions for Claude Design

You are designing the user interface of Quire, an open-source e-reader firmware for the Xteink X3, a pocket e-ink reader. Every screen is a **static, print-like page** on a **528 × 792 px portrait canvas** (3.68 inches at 259 PPI; 10 px ≈ 1 mm). There is **no touch screen**. Input is seven physical keys: four along the bottom edge (Left, Back, Confirm, Right), two on the right edge (Up, Down), and Power on top.

**Thesis: a printed object, not a device.** Quire should look like it was typeset, not programmed. Menus look like the front matter of a beautiful book; numbers look like a poster; the reading page looks like a paperback. It is black ink on white paper, nothing else.

**Six signature elements. Every relevant screen must show them.**
1. **The book is the home screen.** Waking the device shows the page you were reading, already turned. There is no home grid to get past. "Home" is a layer summoned over the page with one press and dismissed with one press.
2. **Edge labels.** Key labels are drawn where the keys physically are: a 40 px rail along the bottom edge with four cells over the four keys, and two short labels on the right edge beside Up and Down. Any overlay that offers choices places them in those same positions (the "compass"), so the hand never has to look for the mapping.
3. **The Spine.** Progress is shown as a book's fore-edge: a vertical strip on the right edge of the reading page made of hairlines, one per ~2 pages, with the read portion drawn solid and the unread portion drawn as fine lines, chapter starts as small notches. Holding Right skims and the Spine becomes the scrubber. It replaces percent bars everywhere progress in a book is shown.
4. **Time, not percent.** Progress is spoken in time: "38 min left in this chapter", "Finish by Thursday". Percent appears only in the Go-to screen.
5. **Phone as window and keyboard.** Any text entry on the device offers "Type on your phone": a QR code that opens the Drop page, which mirrors the device screen and provides a keyboard and remote keys. Text entry with the seven keys still exists but is the fallback.
6. **Jump.** Long-press Back anywhere opens Jump: a single list of everything (recent books, Library, Bookshop, Drop, Stats, Apps, Settings, and every app). It is the launcher, and new features register there, so the UI never needs a deeper hierarchy.

**Hard rules.**
1. Pure 1-bit black on white. No greys, gradients, shadows or transparency. Use hatching and dot screens where you would use grey; use inversion for focus. Only images (covers, photos) may be dithered greyscale.
2. No animation, no motion, no scrolling. Every state is a static page; lists paginate.
3. Focus is inversion of the whole row, cell or button. Never a thin outline alone.
4. Two-press rule: every common reading action (next chapter, contents, type size, dictionary, bookmark, sleep) is reachable within two presses from the page. If a screen breaks the rule, redesign it.
5. Typographic UI. Words before icons. Icons only where a word would not fit (the battery, Wi-Fi, a bookmark). Screen titles in the serif; labels in small caps; large numerals for anything measured.
6. Minimum sizes: body 22 px, captions 18 px, UI strokes 2 px, hairlines 1 px only in the Spine and as inert dividers, focus ring 3 px, rows 56 px, edge-label rail 40 px.
7. One purpose per screen. No badges, toasts, or nested modals. A dialog is a full-width card with at most two actions, mapped to Back and Confirm.
8. Mark every artboard as entering with a partial (DU) or full (GC) refresh.
9. Design the states: default, focused, empty, working (static "Working…" card), error, and inverted dark mode for the reader, Home layer, sleep screens and Jump.

**Type.** UI: Literata (serif) for titles and reading, Atkinson Hyperlegible (sans) for labels and lists, JetBrains Mono for edge labels, times and numbers in tables. Scale in section 4.

**Deliverables.** One artboard per screen and state, named `NN-screen-state`, in the order of section 6, on a 528 × 792 frame with an 8 px grid and 24 px outer margins (the Spine sits inside the right margin). A components board (section 5), a typography board (section 4), a "signature" board showing the six elements in isolation, and a one-page refresh map.

---

## 1. Design direction

**Who.** A reader who bought a $79, 58-gram device that fits in a jeans pocket and lasts weeks, and who is technical enough to flash firmware. They want the device to disappear and the book to remain.

**Thesis.** Every e-reader on the market is a small tablet UI in greyscale: grids of covers, icon toolbars, percent bars, a spinner while it thinks. Quire is a printed object. Its screens are typeset like the front matter, running heads and folios of a well-made book. Its one visual invention, the Spine, turns progress into something you have felt in your hands since childhood: how much of the book is under your right thumb.

**What "wow" is on e-ink.** It is not motion. It is the absence of waiting and the presence of craft. Concretely: the device wakes onto your page in one refresh; a page turn never lags the thumb; the type is better than the paperback's; the menus look like they were set by a typographer; and when you need to type, you pull out your phone and the device shows what you type as you type it.

**Values in priority order.** Legibility → speed (fewest presses, fewest refreshes) → predictability (edge labels everywhere) → completeness → charm.

## 2. Physical model and the key grammar

**Canvas.** 528 × 792 portrait. Four orientations supported; design portrait, plus landscape for the reading page and image viewer.

**Universal key grammar.** The same seven keys mean the same thing on every screen. Overlays present their choices at the key positions (the compass), so a choice is always "the key under that label".

| Key | Everywhere | On the reading page |
|---|---|---|
| Left (bottom, outer left) | Focus left, or previous page of a list | Previous page |
| Back (bottom, inner left) | Back or cancel; **long: Jump** | Short: **Home layer**; long: Jump |
| Confirm (bottom, inner right) | Activate; **long: context actions** | Short: **Compass**; long: **word cursor** |
| Right (bottom, outer right) | Focus right, or next page of a list | Next page; **hold: skim** |
| Up / Down (right edge) | Focus up/down; hold repeats | Previous / next page (remappable to chapter) |
| Power (top) | Short: sleep (configurable: refresh); long: Power menu | Same |

Long-press is 500 ms. Hold-repeat starts at 500 ms and runs at 5 per second. Double-press is not used.

**The two-press rule.** From the reading page: next chapter (hold Right or Down), Contents (Confirm → Left), Go to (Confirm → Right), Type (Confirm → Up), More (Confirm → Down), dictionary (long Confirm, then Confirm on the smart-cursor word), bookmark (Confirm → Down → Confirm, and long-Confirm on the compass toggles it in one), Library (Back → Confirm), sleep (Power). Any new feature must be placed so this stays true.

**Refresh.** A key press changes the screen within 100 ms of firmware time plus one DU refresh (~380 ms). Full GC refresh (~470 ms, a black-white flash) on: entering and leaving the reading page, every N pages (default 10), any screen with an image, dialog open and close. Mark each transition on the artboards.

## 3. Visual language

- **Ink and paper.** #000 on #FFF. Inverted mode swaps globally.
- **Typeset, not drawn.** Screens are built from type, whitespace, and three kinds of line: a 4 px heavy rule (screen title underline, the top of the edge-label rail), a 2 px rule (cards, table heads), and a 1 px hairline (the Spine, inert dividers). Nothing else.
- **Patterns instead of grey.** Disabled: 50% dot screen. Secondary surface: 1 px horizontal hatch at 6 px pitch. Selected-but-not-focused: a 4 px left bar. Focused: full inversion.
- **Corners.** Square. Zero radius anywhere.
- **Numerals.** Anything measured is set large: 44 px Literata numerals with 18 px small-cap labels beneath ("38" / "MIN LEFT"). This is the poster voice and the second most recognisable thing about Quire after the Spine.
- **Small caps labels.** Section labels and edge labels are Atkinson Hyperlegible at 18 px with 0.08 em tracking, in caps. No other caps.
- **Icons.** A set of at most 24: battery ×5, charging, Wi-Fi, Wi-Fi off, bookmark (outline, filled), clock, moon, sun, check, close, chevrons ×4, arrows ×4, QR, lock, warning. 2 px stroke on a 24 px grid. Words do the rest.
- **Covers.** Dithered (Floyd–Steinberg) in a 2 px frame at 152 × 228. Missing cover: a typographic cover, title in Literata 26 px over a hatch, author 18 px.
- **The Spine.** A 12 px wide strip inside the right margin of the reading page, full height minus rail and running head. Composed of 1 px hairlines at 4 px pitch, one per ~2 pages (scaled to the book so it always fills the strip). Read pages: hairlines drawn full-width; unread: drawn 6 px wide, so the read portion looks solid and the unread portion looks like the fanned fore-edge. Chapter starts: a 3 px notch on the outer side. The current position: a 2 px black bar across the full 12 px. In skim, a small label with chapter and time-left rides alongside the bar.
- **QR codes.** Version 3 to 6, module 6 px or larger, 4-module quiet zone, human-readable text beneath.

## 4. Typography scale

UI sans (Atkinson Hyperlegible): 18 label/small caps · 22 body · 26 list title. Serif (Literata): 32 screen title · 44 poster numeral · 56 hero numeral (sleep screen, year in review). Mono (JetBrains Mono): 18 edge labels, times, table numbers. Line height 1.3 for UI.

Reading page (Literata by default, user-selectable): 20 · 22 · 24 · 26 · 28 · 31 · 34 · 38 px, default 26; line height 1.3 / 1.45 / 1.6, default 1.45; margins 16 to 48 px in 8 px steps, default 32 (the Spine sits inside the right margin, so the text block is asymmetric by 12 px, which reads as a book's inner and outer margins). Justified with hyphenation by default; about 37 characters per line and 24 lines per page at defaults.

Chapter openings are typeset like a book: the chapter number as a 56 px numeral, the title in 32 px, a 4 px rule, then a drop cap of three lines on the first paragraph (toggle).

## 5. Component library

Deliver each with focused and disabled variants on the components board.

1. **Running head** (36 px): on non-reading screens the screen title in Literata 32 px sits above a 4 px rule; on the reading page an optional 18 px small-cap running head with book title left and chapter right.
2. **Edge-label rail** (40 px, bottom): four equal cells over the four bottom keys, JetBrains Mono 18 px, a 4 px rule above. Empty cells show a centred 2 px dot so the rail always reads as four positions.
3. **Side labels** (right edge, beside Up and Down): 18 px mono, rotated 90°, only when the side keys act.
4. **Compass overlay**: a card over the lower 40% of the page (the text above is screened with a 50% dot pattern). Four choices placed at the edge-label positions, two at the side-label positions. Each choice: a 26 px label plus one 18 px line of context ("Contents · Ch 12 of 31"). Long-Confirm inside the compass toggles bookmark on this page. Full refresh on open and close.
5. **Home layer**: a card over the upper 60% of the page containing the current book's title, author, "38 min left in this chapter", "Finish by Thursday", then three recent books as one-line rows, then the rail: `Library · Close · Continue · Bookshop`. Side labels: `Drop` / `Stats`.
6. **Jump**: a full page: a search line at the top ("Type on your phone" hint on the right), then an alphabetical list with recent items first. Everything the device can do is a row.
7. **List row** (56 px, or 88 px with thumbnail): title 26 px, second line 18 px, right-aligned value in mono. Focus inverts the row.
8. **Cover grid cell**: 152 × 228 plus two lines, 3 × 2 per page, 6 px frame on focus.
9. **Poster numeral**: 44 px numeral with an 18 px small-cap label beneath; tiles in a 2-column grid with 2 px rules between, no boxes.
10. **Toggle row**, **stepper row**, **choice row**, **slider**: as in a settings list; the stepper shows the value in mono; Left/Right change it when focused.
11. **Dialog card**: full width, 2 px border, 32 px title, 22 px body, two actions on Back/Confirm, always a full refresh.
12. **Working card**: title and a stepped bar with a mono status line; never a spinner.
13. **Empty state**: a 32 px serif line, an 18 px hint, and the rail showing the action that fixes it.
14. **Keyboard**: 3-row QWERTY plus numbers, 48 × 56 px keys, inversion focus, text field with a 2 px caret. Above it, always: a "Type on your phone" strip with a small QR. Also a compact T9 variant for search.
15. **Spine**: as specified in section 3, with the skim label variant.
16. **Peek strip**: pressing Back on the reading page and releasing within 200 ms shows the Home layer; holding Back for 200 to 500 ms instead shows a 48 px strip at the top with chapter, time left in chapter, clock and battery, which disappears on release without a second refresh (drawn and cleared in one DU cycle each). Design the strip.
17. **Specimen**: a live paragraph rendered in the currently selected type settings, used on the Type screen and the first-run "This is your reader" page.
18. **Phone window**: not on the device, but design it in the same system for the Drop page: a 1:2 rendering of the device screen with seven on-screen keys around it and a keyboard beneath.

## 6. Screen inventory

Design order. Required states for each: default, focused, empty, error where applicable; dark mode for 10, 20, 21, 40, 60a.

**01 Boot** — Wordmark set in Literata, version in mono, one stepped bar ("Indexing 212 books"). At most 2 s, GC only.

**02 First run** — Four pages: language; time (auto from Wi-Fi later, manual now); "This is your reader" specimen page with the Spine drawn and a one-line explanation of the six signatures in plain words; "Add books" offering Bookshop (Start here picks) and Drop (QR). A sample book ships in flash so the device reads before anything is added.

**10 Home layer** — Over the current page (component 5). Also the variant when no book is open: a full page titled "Nothing open yet" with three Start-here picks from the Bookshop and the Drop QR.

**11 Library** — Running head "Library", then a 2 px-ruled tab line: Recent · All · Authors · Series · Collections · Folders (Left/Right switch when the tab line is focused). Body: cover grid or list (toggle in Jump or long-Confirm). Page number in mono at the right of the tab line. Long-Confirm on a book: compass with Open · Info · Finished · Collection, side: Delete / Move.

**12 Book info** — Cover left, title 32, author, series, then poster numerals: `4 h 12` TIME LEFT · `Thu` FINISH BY · `62%` READ (the one place percent is allowed alongside), then tags, format, size, added. Rail: `Back · — · Open · More`.

**13 Folders** — SD browser, breadcrumb in mono.

**20 Reading page** — Text block, the Spine, optional running head, no rail. Design at 22, 26 and 34 px and at both line-height extremes. Show: chapter opening with numeral and drop cap; a page with an inline image; a page with a footnote marker; verse; a code block; a stacked table; the last page of a book; landscape.

**21 Compass** — Component 4 with the reading choices: Left `Contents`, Right `Go to`, Up `Type`, Down `More`, Back `Close`, Confirm `Bookmark` (label reads `Bookmarked ✓` when set). Design the More page as a second compass: Left `Dictionary`, Right `Highlights`, Up `Layout`, Down `Sleep`, Confirm `Stats`.

**22 Contents** — Nested list, current chapter marked with a filled bookmark, each row's right value is time-to-read that chapter in mono ("41 min"). The Spine is drawn at the right with the current position, so the list and the fore-edge agree.

**23 Go to** — A large 44 px `43%` numeral with Left/Right stepping, a chapter picker, and "Page 123 of 412". Hint: hold Right on the page to skim.

**24 Skim** — The reading page keeps turning under a held Right; the Spine's position bar moves and a small label rides beside it: `Ch 14 · 51% · 2 h 03 left`. Release lands. Show three frames.

**25 Type and Layout** — Two screens. Type: a specimen at the top half that re-renders on every change; below it, font family rows (each name set in its own face), size stepper, weight/darker toggle, anti-alias toggle. Layout: line height, margins, alignment, hyphenation, paragraph style, drop caps, embedded styles, publisher fonts, running head, Spine on/off, full-refresh cadence, profiles.

**26 Word cursor** — Long-Confirm on the page: the cursor lands first on the **rarest word on the page** (by frequency list), because that is the word you most likely want; Left/Right cycle by rarity, Up/Down move by line, Confirm looks it up, long-Confirm starts a selection that grows with Right. Cursor: 2 px underline plus a small mono `1/6` counter at the top right. Show a selection over two lines.

**27 Dictionary** — Headword 32 px, pronunciation 22 px, definitions paginated, rail: `Back · Save word · Wikipedia · Next dict`.

**28 Highlights and notes** — Grouped by chapter; the highlighted sentence in serif 22 px, a note beneath in sans 18 px; notes are typed on the phone. Rail: `Back · Open · Note · Delete`.

**29 Footnote** — Card over the lower third; `Back to text`.

**2A End of book** — Poster page: `6 h 40` READING TIME · `9 days` START TO FINISH · `28` PAGES / HOUR; then Rate (five focusable stars), Finished, Next in series with cover, Library.

**30 Drop page (device)** — Title "Drop books here", the QR pair, URL in 26 px mono, live list of arriving files with stepped bars, and a line "Wi-Fi: HomeNet · 3 books added". Variants: hotspot (name and password in text), not connected, complete, disk full.

**31 Wi-Fi** — Saved networks with signal bars, Add network → scan list → password ("Type on your phone" shown first, keyboard as fallback), status card with `.local` name and IP.

**32 OPDS catalogs** — Saved servers, catalog browser, download working card.

**33 Calibre connect** — Status page: "Waiting for Calibre… 192.168.1.20:9090", stepped activity, transfer list when connected.

**34 Sync** — Per-book position sync status, last sync, Sync now, server settings.

**35 Bookshop home** — Six shelves, each a row of three covers plus More: Start here · Popular this week · New editions · Modern & Creative Commons · Collections · By subject. Search at the top with the phone hint. First-visit line: "Free, open books. No account, no DRM."

**36 Book page (Bookshop)** — Cover, title, author, year, language, poster numeral `6 h` TO READ, source and licence line, blurb paginated. Primary Get → stepped bar → Read. Secondary: Save for later, More by this author, Same collection. States: not downloaded, downloading, in library, error ("Gutenberg is limiting requests, retrying in 30 s").

**37 Bookshop search** — Results from the offline catalog as you type (phone or keyboard), sorted by popularity, source glyph per row; works with Wi-Fi off, with a "Connect to get this book" state.

**38 Bookshop browse** — Subjects, Authors A to Z, Collections with one-line intros, Languages.

**39 Downloads and Saved** — Queue with progress and errors; Saved list with Get all.

**40 Sleep screens** — Cover: full-bleed dithered cover with a bottom band: title, `38 min left in this chapter`, and a miniature Spine; Poster: `Thursday` FINISH BY with the title beneath in 26 px; Quote of the day; Quick resume (the page screened at 50% with a moon glyph and "Press Power"); Custom image; Charging (56 px battery percent); Empty battery.

**41 Power menu** — Sleep, Power off, Restart, Refresh, Wi-Fi on/off, Lock keys, on the compass layout.

**44 Sleep screen picker** — Reached from Settings → Sleep and power, from Jump ("Sleep screen"), and from the Power menu. Six live thumbnails at 1:4 scale rendered from the current book (Cover, Poster, Quote, Custom, Quick resume, Blank) in a 3 × 2 grid; Left/Right/Up/Down focus, Confirm applies (a small "in use" check under the thumbnail). Beneath the grid, options for the focused variant: Cover → title band on/off; Poster → Finish by or Streak; Quote → built-in or `quotes.txt` on SD; Custom → folder, rotation (fixed / each sleep / daily); Quick resume → moon glyph on/off. Rail: `Back · — · Use · Preview`; Preview shows the full-size screen for 3 s then returns. Custom images are added by dropping photos into the Drop page's "Sleep images" section or the `/sleep` folder on SD; the device dithers them to 528 × 792 at ingest.

**45 Keys locked** — When Lock keys is on (Power menu, or a setting "lock when sleeping in a pocket"), the current page stays on screen with a small lock glyph in the running head. Any key press draws a one-line strip at the top for one refresh: "Keys locked · hold Power to unlock". Hold Power 2 s unlocks with a single DU. Also design the locked state over the sleep screen (lock glyph in the band).

**42 Jump** — Component 6. Show it with a query typed from the phone filtering the list.

**43 Phone window (in the Drop page, for reference)** — The device screen mirrored at 1:2 with seven keys drawn around it in the physical positions, a text field and keyboard beneath, and "Sent to reader" feedback. Design in the same 1-bit system so it feels like one product.

**50 Settings** — Groups as a list: Reading, Display, Keys, Sleep and power, Wi-Fi and sync, Library, Bookshop, Language and time, Apps and games, About, Developer. Design Keys (a line drawing of the device with each key labelled and remappable, side-key swap, orientation follow), Sleep and power (timeouts, a row that opens 44 Sleep screen picker, key-lock options, panel-off, battery with a `19 days` LEFT numeral), About (version, storage bars as hatched rules, Check for update, licences).

**51 OTA update** — Available version with paginated notes, Install, working card, Restart dialog, SD-update and recovery variants.

**60 Analytics** — A five-page section reached from Jump, the Home layer's side label, and Book info. All charts are 1-bit: bars are solid ink, secondary series are hatched, grids are hairlines, and the current value is labelled with a poster numeral. Never a legend; label the marks directly.
- **60a Overview.** Tabs Today · Week · Month · Year · All (Left/Right). Six poster numerals in a 2 × 3 grid: `1 h 42` READ · `86` PAGES · `31` PAGES / HOUR · `12` STREAK DAYS · `2` BOOKS FINISHED · `4 h 10` LEFT IN CURRENT BOOK. Beneath: the period as an **ink line**, a 1-bit bar chart with one bar per hour (Today), day (Week/Month), or month (Year), the tallest bar labelled. Side labels: `Books` / `Goals`.
- **60b Rhythm.** When you read: a 24-column time-of-day histogram (all time), a 7-column weekday chart, and `9 pm` FAVOURITE HOUR · `Sun` FAVOURITE DAY · `27 min` TYPICAL SESSION. Then a session list for the selected day in mono: start, length, book.
- **60c Calendar.** The month as a 7-column heat map of dot screens (0 / 25 / 50 / 100 % of the daily goal), today outlined, streak days joined by a 2 px rule; poster numerals `12` CURRENT STREAK · `41` LONGEST · `19 / 30` DAYS THIS MONTH. Left/Right change month.
- **60d Books.** A table of every book with time, pages, pages per hour, started and finished dates, sortable by long-Confirm on a column head; a row opens Book info, whose analytics block shows that book's own ink line and `Thu` FINISH BY forecast (from the last seven sessions' pace).
- **60e Goals.** Daily goal (minutes or pages) and yearly goal (books) as steppers; progress as a hairline Spine that fills; a quiet awards list (first book, 7-day streak, 100 hours, a night owl badge) rendered as small-cap lines, never popups.
- The phone window (43) has an **Analytics tab** with the same data in full: interactive charts, export CSV/JSON, a StoryGraph/Goodreads CSV, and a printable year poster. The device stays 1-bit; the phone gets the rich version.

**61 Year in review** — A poster: `31` BOOKS · `212 h` READ · `9 pm` FAVOURITE HOUR, five small covers, the longest session, the longest streak. Shareable as a screenshot.

**70 Apps** — A typographic list, not an icon grid: Clock, Flashcards, News, Wikipedia, Weather, Calculator, Notes, Images, Interactive fiction; Games as a second page. Each row has a one-line description.

**71 Flashcards** — Deck list; card front (serif, centred, large); back; rating on the rail: `Again · Hard · Good · Easy`; session summary as poster numerals.

**72 News** — Feeds with unread counts; articles as rows; reading uses the reading page with the Spine.

**73 Clock and timer** — 56 px time, date in small caps; pomodoro as a 2 px stepped ring; alarms (visual only).

**74 Weather** — `21°` numeral, condition in words, five-day row of numerals.

**75 Image viewer** — Full-bleed dithered image; Left/Right browse; Confirm toggles fit.

**76 Interactive fiction** — Transcript on the reading page; a command line; verb row and noun grid on the compass so common commands need no typing; phone keyboard for the rest.

**80 Games** — List with descriptions. Boards use the inner 480 px width and inversion cursors. Design Sudoku (52 px cells, 14 px pencil marks), 2048 (112 px tiles, numerals in Literata), Minesweeper (30 px), Chess (60 px, hatched dark squares, clear piece glyphs), Wordle (72 px tiles, T9 keyboard with state marks). Each with a paused card.

**90 Developer** — Mono throughout: key ADC readings, panel type and timings, heap graph as a stepped line, SPI benchmark, log tail.

**99 Recovery** — 32 px text, two actions, no icons.

## 7. Speed as a design requirement

The design promises instantness; the firmware must deliver it, and the artboards should assume it.

| Moment | Budget | How |
|---|---|---|
| Wake from light sleep to the page | 1 refresh, under 500 ms | The page is still in the controller's RAM; only a DU refresh runs |
| Wake from deep sleep to the page | under 2 s | Quick-resume image is the page itself; the layout engine restores position from the page index |
| Page turn | one DU, ~380 ms, zero layout wait | Page N+1 is always pre-rendered into a shadow plane during idle; a turn only kicks the waveform. Presses during a refresh are queued, never lost |
| Compass, Home layer, Peek | one DU | Overlays are composed onto the cached page plane, no relayout |
| Skim | 5 pages per second | Pre-rendered pages from the index; only the Spine label changes per frame |
| Font size change | under 1 s to the new page | Relayout only the current chapter, index the rest in the background |
| Word lookup | 2 presses, under 1 s | Frequency list in flash; dictionary index on SD |
| Jump | 1 long-press, one DU | List is static; typing from the phone filters without a refresh storm (at most 2 refreshes per second) |
| Bookshop search | as you type | Offline index on SD; no network until Get |

## 8. Flows to storyboard

1. **Out of the box to reading**: 01 → 02 (four pages) → the sample book opens on 20. Under 90 seconds, no computer, no typing.
2. **Wake and read**: Power → 20. Zero presses.
3. **Find your place**: 20 → hold Right (24) → release. One press.
4. **Look up a word**: 20 → long Confirm (26, cursor already on the rare word) → Confirm (27) → Back. Three presses.
5. **Change the type**: 20 → Confirm → Up (25, specimen updates live) → Back. Three presses, one GC on return.
6. **A new book from the phone**: 20 → Back (10) → Bookshop or Drop → phone → new book opens.
7. **A free book with no computer**: 20 → Back → Bookshop (35) → Popular → Get → Read.
8. **Type from the phone**: 42 Jump → "Type on your phone" → phone window (43) → text appears on the device as typed.
9. **Finish a book**: last page → Right → 2A → Next in series → 20.

## 9. Copy and tone

Short, plain, warm, typeset. Sentence case. Numerals for numbers. Time before percent. No exclamation marks, no emoji, no "Oops". Errors say what happened and what to do: "Couldn't join HomeNet. Check the password." Labels on the rail are one word where possible: `Open`, `Get`, `Close`, `Contents`.

## 10. Accessibility and comfort

Large UI mode (×1.25 scale, 72 px rows); inverted mode; left-handed rotation with edge labels following the keys; Simple mode hiding Apps and Games from Jump; key lock in the Power menu; the Spine can be hidden; drop caps and running head are toggles.

## 11. What to send back

- Artboards named per section 6, exported at 1:1 (528 × 792) and as 1-bit PNG.
- The components board, typography board, and a **signature board** with the six elements isolated (Home layer, edge labels and compass, Spine in three states, a poster numeral group, the phone window, Jump), and a **charts board** showing the ink line, the histogram, the heat map and the goal Spine at 1:1 in 1-bit.
- The refresh map.
- Notes on any screen where the two-press rule or the key grammar felt wrong, with a proposed alternative.
