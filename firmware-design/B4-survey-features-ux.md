# Appendix B4 — E-reader features and e-ink UX survey (agent report, 2026-09-13)

Verbatim report from the targeted feature and UX research pass.

Hardware baseline used throughout: ESP32-C3 (RISC-V, ~380 KB usable RAM, 16 MB flash), 3.68" 792x528 panel (~259 PPI), 1-bit rendering in practice (panel/controller can do 4 grays via two RAM planes but stock firmware never implements it), UC8253/UC8279 controller (~21-42 ms frame transfer), DS3231 RTC, QMI8658 IMU (tilt/shake page turn), BQ27220 fuel gauge, 2.4 GHz Wi-Fi, microSD, pogo-pin charging, no frontlight, no touch, no speaker. Buttons: four along the bottom edge (Left, Center-Left, Center-Right, Right), two on the right side (Up/Down), a Power button, and a recessed Reset. Sources: [papyrix X3 spec](https://github.com/bigbag/papyrix-reader/blob/main/docs/x3-specifications.md), [X3 EPD analysis gist](https://gist.github.com/CrazyCoder/82fec0bbd0e515dcc237d3db7451ec6f), [CrossPoint README](https://github.com/crosspoint-reader/crosspoint-reader), [CrossPoint USER_GUIDE](https://github.com/crosspoint-reader/crosspoint-reader/blob/master/USER_GUIDE.md).

Note on sourcing: several sites (news.ycombinator.com, withintent.com, pocketink.io, sixcolors.com, ereadersforum.com, kobo.com, mudita.com) were blocked by the egress proxy, so search-result excerpts were used for those and GitHub-hosted docs for everything else.

## 1. Feature catalog by category

### Typography and layout
- KOReader: embedded EPUB stylesheet and fonts, font hinting, font gamma, hyphenation, kerning, pagination, DPI control, landscape reading, style tweaks. ([KOReader Features list](https://github.com/koreader/koreader/wiki/Features-list))
- CrossPoint (the reference for this hardware): font family (Noto Serif default, Noto Sans, SD-card custom fonts), 4 sizes (S/M/L/XL), line spacing (Tight/Normal/Wide), margin 5-40 px in 5 px steps, alignment (Justified/Left/Center/Right), embedded-CSS on/off, hyphenation on/off, paragraph spacing vs first-line indent, anti-aliasing on/off (grey edges, slightly slower), Focus Reading (Bionic-style bolding), 4 orientations. ([USER_GUIDE](https://github.com/crosspoint-reader/crosspoint-reader/blob/master/USER_GUIDE.md))
- papyrix: 4 sizes, layout presets (indent/spacing), hyphenation in 6 languages, .epdfont custom fonts, SD-card themes, "Turbo LUTs" for faster X3 page turns, sunlight-fade prevention by powering the panel down. ([papyrix README](https://github.com/bigbag/papyrix-reader))
- CrossInk adds Bionic Reading, guide dots between words, better paragraph indents; Witch(hunt) Reader adds more faithful CSS. ([PocketInk comparison](https://pocketink.io/blog/xteink-firmware-comparison-crosspoint-crossink-microreader-papyrix/), [CrossPoint README forks section](https://github.com/crosspoint-reader/crosspoint-reader))

### Page turning and navigation
- KOReader: partial refresh, flipping/skim mode, follow links, in-page footnotes, TOC with alternative heading extraction, text search, go-to. ([Features list](https://github.com/koreader/koreader/wiki/Features-list))
- CrossPoint: long-press Right/Side-Down = next chapter (or "page scroll" hold mode), auto page-turn in pages/minute, tilt page turn (X3 gyro), footnote jump with position restore, percentage go-to, orientation cycle, QR code of current position, screenshot. ([USER_GUIDE](https://github.com/crosspoint-reader/crosspoint-reader/blob/master/USER_GUIDE.md))
- Kindle: Page Flip (slider plus page preview, position restored), Time to Read (per-chapter and per-book estimate from your own speed). ([Kindle features](https://www.amazon.com/b?ie=UTF8&node=17717476011), [justkindlebooks](https://www.justkindlebooks.com/article_jkb/what-are-kindle-reading-insights/))

### Status bar / footer
- KOReader footer items: page number, percent, pages left in chapter, chapter name, time left in chapter, time left in book, clock, battery, memory, Wi-Fi; progress bar with TOC chapter markers; top or bottom placement. ([KOReader issue #12572](https://github.com/koreader/koreader/issues/12572), [Features list](https://github.com/koreader/koreader/wiki/Features-list))
- CrossPoint: None / No Progress / Full with Percentage / Full with Book Bar / Book Bar Only / Full with Chapter Bar; battery percentage hidden Never / In Reader / Always. ([USER_GUIDE](https://github.com/crosspoint-reader/crosspoint-reader/blob/master/USER_GUIDE.md))
- Kobo: footer per chapter or book, as page number, percent remaining, or time remaining based on reading history; same figure shown on the sleep screen. ([Goodreads Kobo thread](https://www.goodreads.com/topic/show/12062730-can-the-information-displayed-in-the-footer-be-customised))

### Reading statistics and motivation
- KOReader statistics plugin: time spent, pages read, average time per page (capped, default 120 s; page-turn timeout 5 s), estimated time to finish, pages/day, all-books totals, weekly/daily summaries, calendar month grid with 24-bar hourly histogram, "today's timeline", SQLite export/sync. ([statistics main.lua](https://github.com/koreader/koreader/blob/master/plugins/statistics.koplugin/main.lua), [Calendar view PR](https://github.com/koreader/koreader/pull/5854))
- Streak plugin: daily and weekly streaks, longest streak, page/time thresholds a day must meet, streak goal 1-365 days with congratulation. ([readingstreak.koplugin](https://github.com/advokatb/readingstreak.koplugin))
- Kindle Reading Insights: streaks, days read per week/month, seasonal challenges with 15 mystery badges, Goodreads challenge progress. ([Reading Insights](https://www.justkindlebooks.com/article_jkb/what-are-kindle-reading-insights/), [mybookjoy](https://mybookjoy.com/2025/01/02/kindle-2025-new-year-reading-challenge-achievements-guide/))
- Kobo Reading Life: books read, pages turned, reading speed, times of day, average session, pages per minute, spontaneous awards; StoryGraph sync for streaks/challenges. ([Kobo blog](https://www.kobo.com/blog/https-news.kobo.com-your-reading-activity-explained), [Android Authority](https://www.androidauthority.com/kobo-storygraph-integration-live-3682595/)). Readers complain that date-completed, average time per book and pages remaining are missing ([the-ebook-reader](https://blog.the-ebook-reader.com/2025/05/19/ebook-readers-lack-useful-reading-history-and-reading-stats/)).
- CrossPet X3: per-session/daily/all-time stats and a Tamagotchi chicken fed by pages read (20 pages per meal, falling to 10 at a 30-day streak). ([crosspet-x3](https://github.com/bcrpntr/crosspet-x3/))

### Lookup, annotation, sync
- KOReader: StarDict dictionaries, Wikipedia lookup and save-as-EPUB, translation, highlights, bookmarks, night mode, screensaver, light control, profiles, gesture manager, Calibre wireless, OPDS, progress sync. ([Features list](https://github.com/koreader/koreader/wiki/Features-list))
- Kindle: X-Ray (characters/places), Word Wise inline hints, Vocabulary Builder with flashcards, Whispersync. ([Kindle features](https://www.amazon.com/b?ie=UTF8&node=17717476011), [Word Wise guide](https://www.justkindlebooks.com/article_jkb/how-to-use-word-wise-and-vocabulary-builder-on-kindle/))
- Kobo: Beyond the Book, built-in translation, beta games (Sudoku, Solitaire, Unblock It, Word Scramble, Sketch Pad). ([PopSci](https://www.popsci.com/diy/kobo-tips/))
- PocketBook: Dictionary, Calculator, Notes, RSS News, Chess, Klondike, Sudoku, Scribble, TTS. ([MobileRead PocketBook apps](https://wiki.mobileread.com/wiki/PocketBook_apps)) Boox: RSS, TTS, split screen, third-party dictionary hooks. ([BOOX dictionary post](https://medium.com/boox-content-hub/third-party-dictionary-app-integration-on-neoreader-fcf6ad1d4d2))
- CrossPoint: StarDict lookups, up to 8 OPDS servers, Calibre wireless, KOReader sync (default server sync.crosspointreader.com), WebDAV, web UI uploads, OTA, sleep-screen modes (Dark/Light/Custom/Cover/Cover+Custom/Transparent/None/Quick resume), full refresh every 1/5/10/15/30 pages.

### CrossPoint app ecosystem
- crosspoint-reader-apps: Markdown/HTML parser, Calculator, Weather (cached), Sudoku, Wikipedia (text-only to SD), Chess engine, Dice and 8-ball, RSS (offline cache), Reddit, DuckDuckGo. ([crosspoint-reader-apps](https://github.com/zakerytclarke/crosspoint-reader-apps))
- CrossMux (X3/X4): Sudoku, Gomoku, Xiangqi, Minesweeper, 2048, Electronic Woodfish counter, Ugly Avatar generator, AirPage image push, WeRead, clock and almanac standby faces. ([crossmux](https://github.com/0x1abin/crossmux))
- CrossPet X3: Chess with AI levels, Gomoku/Caro, Sudoku, Minesweeper, 2048, virtual pet.
- CrossPlay (X4 Pro/S3 only, not X3): 21 still-screen games incl. Chess, Checkers, Go, Picross, Solitaire, Yahtzee, Trivia (50k Jeopardy), plus Anki/FSRS flashcards, Hacker News, xkcd, Instapaper, Wikipedia, ESP-NOW two-device play. Its philosophy line is worth stealing: "E-ink is good at waiting. So is a chess position, a flashcard, a puzzle you are halfway through." ([crossplay](https://github.com/ma-r-s/crossplay))

### Stock X3 firmware and why people leave it
Formats .epub/.xtc/.xtch/.txt/.bmp, font install via SD, phone-app transfer, shake/tilt page turn. Weaknesses: fixed margins, generic default font, basic and narrow feature set, no stats, no Calibre/OPDS; reviewers say the device people rave about is running CrossPoint. ([Six Colors](https://sixcolors.com/post/2026/07/review-xteink-x3-is-the-little-e-reader-the-worlds-not-quite-ready-for/), [PocketInk](https://pocketink.io/blog/xteink-x3-what-i-wish-i-knew/), [eReadersForum](https://www.ereadersforum.com/threads/xteink-x3-stock-firmware-vs-crossing-what-actually-pushes-people-to-switch.13612/))

## 2. E-ink UX rules (actionable)

1. Treat the screen as print, not a slow monitor: no animations, transitions, spinners or fades; a stepwise progress bar is acceptable. ([HN thread](https://news.ycombinator.com/item?id=49213660), [eink-css-ui-framework](https://github.com/marcomattes/eink-css-ui-framework))
2. Page, never scroll. Lists move one row/page at a time.
3. Partial refresh for page turns and menu cursor moves; force a full (flashing) refresh every N pages (CrossPoint default options 1/5/10/15/30) and always after images, dialogs, inverted regions or big dark areas, which ghost worst.
4. Offer a manual "Refresh" action (CrossPoint binds it to a short power press).
5. Batch updates: draw the whole new screen once, never incrementally.
6. Design for 1-bit first. Mid-tone grey and alpha dither unpredictably between refreshes; use flat fills, hatch patterns for disabled, and inversion for selected/pressed.
7. Minimum stroke 2 px for UI lines and icons; 1 px only for inert dividers; 3 px focus ring with 2 px offset; square line caps.
8. Focus must be unmistakable for button navigation: invert the whole focused row, not a thin outline. No hover exists.
9. Dither images with Floyd-Steinberg or ordered dither at render time; never rely on panel grayscale alone.
10. Minimal modals: prefer full-screen pages over popups; a popup must be redrawn with a full refresh on open and close to avoid a ghost rectangle.
11. Footer: one line, small sans, percent or time-left plus chapter; clock and battery optional and hideable in reader.
12. Body type: screen-tuned serifs with large x-height and good hinting (Bookerly, Literata, Bitter, Merriweather, Noto Serif, Charter); default around 10-11 pt equivalent at 259 PPI; keep line-height ~1.4-1.6 for prose. ([pdf.net fonts](https://pdf.net/blog/best-e-book-fonts), [ebook-fonts repo](https://github.com/nicoverbruggen/ebook-fonts))
13. Anti-aliasing is a tradeoff on 1-bit panels: either render at 2-bit with dithered edges or pick fonts with sturdy strokes; expose it as a toggle as CrossPoint does.
14. No notifications, no badges that pull attention; one purpose per screen. ([withintent](https://www.withintent.com/blog/e-ink-design/))
15. Sleep screen is a feature: cover, custom art, "quick resume" showing the last page with a moon glyph, or the Kobo-style progress number.
16. Battery: deep sleep after 1-30 min inactivity, wake on Power GPIO; power the panel down when idle (papyrix does this to stop sunlight fading); keep Wi-Fi off unless an action needs it.

## 3. Button mapping recommendations (6 buttons + power)

| Input | Reader | Menus / apps |
|---|---|---|
| Bottom Left / Right | Prev / Next page | Move cursor left/right (or up/down in lists) |
| Side Up / Down | Prev / Next page (swappable, disableable) | Move cursor up/down; long-hold = fast scrub |
| Center-Left (Back) | Short: home; long: file browser | Back / cancel |
| Center-Right (Confirm) | Short: reader menu; long (0.4 s): bookmark, dictionary or KOSync (user choice) | Select |
| Long-hold Right/Side-Down | Chapter skip, or continuous page scroll (hold-to-skim) | Page through long lists |
| Power short | Configurable: ignore / sleep / page turn / refresh / footnotes | Same |
| Power double/triple (CrossPet) | Stats, star page, auto-turn, sync time | - |
| Power + Side Down | Screenshot | - |
| Reset + Back + Power | Bootloop escape | - |

Accessibility: every front button independently remappable; side buttons swappable; four orientations so a left-hander flips the device and the side buttons land under the left thumb; quick-return option makes Power act as Back inside footnotes. In grids (sudoku, chess, minesweeper) use Left/Right for column, Side Up/Down for row, Confirm to place, long-Confirm for a mode switch (pencil marks, flag). Text entry: a 3-row on-screen keyboard cycled with Left/Right and Up/Down, or a T9-style multi-tap grid; keep it rare.

## 4. Games and apps feasibility (ESP32-C3, 1-bit 528x792, buttons only)

| App | Feasibility | Note |
|---|---|---|
| Sudoku | High (shipped in 4 forks) | 9x9 grid fits at 55 px cells; generator is cheap |
| Chess vs engine | High (CrossPoint apps, CrossPet with AI levels) | Depth-2-3 minimax fine on C3; cursor-select two-step |
| Checkers | High | Same UI as chess, simpler engine |
| 2048 | High (CrossMux, CrossPet) | 4 directional buttons map perfectly |
| Minesweeper | High (CrossMux, CrossPet) | Long-Confirm to flag |
| Wordle | High | Offline word list; 5x6 grid; keyboard cycling is the only friction |
| Crossword | Medium | Needs .puz parser and letter entry; use cycling keyboard per cell |
| Solitaire | Medium (CrossPlay has it on S3) | Many cursor targets; workable with column-select then card-select |
| Tic-tac-toe, Hangman | High | Trivial; hangman letter picker is a 26-cell grid |
| Text adventure (Z-machine) | High | jzip-derived AZIP runs on ESP32; dynamic memory under 48 KB; stream story from SD; typing is the pain, so add verb/noun pickers and command history ([AZIP_ESP32](https://github.com/talofer99/AZIP_ESP32), [intfiction memory](https://intfiction.org/t/z-machine-dynamic-memory-64k-is-too-much/59850)) |
| Trivia | High | Multiple choice only; question packs on SD |
| Flashcards / spaced repetition | High (CrossPlay uses Anki + FSRS) | Reveal then rate 1-4 on four bottom buttons |
| Pomodoro / timer | High | RTC on board; screen only redraws each minute |
| Calendar / clock / alarm | High for display, Low for alarm | No speaker; "alarm" can only be a wake-and-flash |
| Weather | High (needs API) | Fetch and cache; render 1-bit glyphs |
| RSS / news to text | High (CrossPoint apps) | Strip HTML, paginate like a book |
| Wikipedia lookup | High | Text-only, saved to SD |
| Hacker News | High (CrossPlay) | Algolia API; comments paginated |
| Quotes | High | Local file; doubles as sleep screen |
| Calculator, unit converter | High | Cursor over a keypad grid |
| Notes | Low | Button keyboard is painful; limit to short tags or import via web UI |
| Audio | Not feasible | No speaker; BT present on SoC but no audio path in firmware |

## 5. Stats and motivation worth copying

Per-book: time, pages, avg time/page, estimated time to finish book and chapter (Kindle Time to Read), date started/finished, pages remaining. Global: daily/weekly totals, pages per hour, time-of-day histogram, calendar heatmap, streak with threshold and longest-streak, yearly goal with progress ring, session list. All computed from a session log on SD; KOReader's 5 s min / 120 s max page-time capping keeps numbers honest.

## 10 delight ideas

1. Sleep screen showing the book cover plus a one-line "42 min left in chapter" and today's streak count.
2. "Hold to skim" thumbnail strip: hold Right to fan through pages at 5/sec with a partial-refresh position bar, release to land (Kindle Page Flip, buttons only).
3. Vocabulary Builder: long-Confirm on a word saves it; a flashcard app reviews saved words with FSRS.
4. Reading Life awards done tastefully: quiet badges on the stats page only, never popups.
5. Year-in-review page rendered as a 1-bit poster: books, pages, longest streak, favorite hour.
6. Z-machine interpreter with verb/noun pickers, so Zork is playable with six buttons.
7. QR code of the current position or a highlight, scanned with the phone (CrossPoint already has position QR).
8. Two-device chess or battleship over ESP-NOW between X3s (CrossPlay's Play Nearby idea).
9. Daily "front page" fetch: one RSS/HN digest paginated as a book, available when you wake up.
10. Reading pet or plant that grows with pages (CrossPet) with an opt-out for people who hate gamification.
