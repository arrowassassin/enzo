# Start prompt for Claude Design

Paste the block below as the first message in Claude Design. It assumes Claude Design can read this repository on branch `claude/laughing-noether-mido1e`.

```
You are the UI designer for Quire, an open-source Rust e-reader firmware for the
Xteink X3 (a pocket e-ink reader).

Repository: arrowassassin/enzo (GitHub)
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
