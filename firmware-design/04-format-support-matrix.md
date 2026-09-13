# 04 — Format support matrix

"Every reading format" is delivered by two engines: the **on-device** engine (streaming parsers that compile to QTX token streams) and the **converter** (desktop CLI and in-browser WASM) that produces `.qbk`. The user experience is identical: drop a file on the Drop page; if the device cannot read it natively, the page offers a one-tap conversion in the browser and uploads the result.

Legend: **Native** = parsed on the device · **Convert** = via the converter to `.qbk` · **Both** = native with a better result via the converter.

| Format | Path | Tier | On-device approach and limits | Converter approach |
|---|---|---|---|---|
| EPUB 2 / EPUB 3 (DRM-free) | Both | T0 | rawzip + miniz_oxide streaming; xmlparser tokeniser; supports headings, emphasis, lists, quotes, verse, code, tables (stacked), images, footnotes, TOC (NCX and nav), ruby. CSS is reduced to a small property set (font-style, weight, align, margins, page-break). Fixed-layout EPUBs are treated as flow with images. | Full CSS cascade and publisher fonts; fixed-layout EPUB rendered as FIXD pages |
| KEPUB (Kobo) | Native | T1 | Same as EPUB; Kobo span markup is ignored | — |
| TXT (UTF-8, UTF-16, Latin-1, GB18030 via SD table) | Native | T0 | Lazy page index; paragraph detection; optional "smart" reflow of hard-wrapped Gutenberg text | — |
| Markdown (.md) | Native | T1 | pulldown-cmark no_std → QTX | — |
| HTML / XHTML single file | Native | T1 | Same tokeniser as EPUB chapters; external resources ignored | Full page with images via converter |
| FB2 / FB2.zip | Native | T1 | Plain XML; sections, titles, epigraphs, poems, notes, base64 images | — |
| MOBI (KF7, PalmDOC) | Native | T2 | In-house PalmDOC decompressor + HTML strip; DRM-free only | Calibre-quality via converter |
| AZW3 / KF8 | Both | T2 | HUFF/CDIC decompression + KF8 skeleton/fragments; heavier, ships after KF7 | Recommended path |
| AZW / KFX with DRM | Not supported | — | No DRM, ever | — |
| CBZ | Native | T1 | rawzip listing; JPEG/PNG decoded at ingest into pre-scaled 2-bit pages; panel zoom (2 × 2 quadrants); right-to-left reading order option | Converter pre-crops, denoises and dithers at higher quality |
| CBR / CB7 / CBT | Convert | T1 | RAR needs libunrar (C++) | Converter unpacks and emits FIXD |
| Image folders (JPG, PNG, BMP, QOI) | Native | T1 | Same as CBZ | — |
| PDF (text or scanned) | Convert | T1 | Not realistic in 380 KB RAM; no no_std renderer exists | pdfium (CLI) / pdf.js (web) → FIXD pages at 528 × 792 with margin crop, 2-bit dither, plus extracted text for search; reflow to FLOW when the text layer is clean |
| DjVu | Convert | T2 | No Rust decoder | djvulibre (CLI only) → FIXD |
| DOCX | Convert | T1 | Zip + XML is feasible but styles and tables make it a poor on-device experience | docx parsing on the desktop → FLOW |
| RTF | Convert | T2 | Skip on device | rtf-parser → FLOW |
| ODT | Convert | T2 | — | Same as DOCX |
| CHM | Convert | T2 | — | chmlib → FLOW |
| LIT, PDB (eReader), TCR, PRC (non-Mobi) | Convert | T3 | Legacy | Calibre-style converters; low priority |
| CBZ of webtoons (tall images) | Native | T2 | Vertical panel strip mode | — |
| Plain code files (.rs, .py, .c, …) | Native | T2 | Mono font, line numbers, no wrap option | — |
| CSV / JSON (for flashcards, trivia packs, word lists) | Native | T2 | Data for apps, not reading | — |
| StarDict (.ifo/.idx/.dict/.dz/.syn) | Native | T1 | Dictionary data | Converter can build from other dictionary formats |
| `.qbk` (our format) | Native | T0 | FLOW and FIXD; the fast path for everything | Produced by the converter |
| XTC / XTCH (Xteink pre-rendered) | Native (read-only) | T2 | Small parser for compatibility with existing user libraries | Converter can produce `.qbk` from XTC |
| OPDS feeds, RSS/Atom | Native | T1 | Network content rendered through the HTML path | — |

## Rendering quality targets

| Element | On-device (FLOW) | Converter (FIXD) |
|---|---|---|
| Body text | Justified, hyphenated, kerned bitmap strikes at 8 sizes | Same engine, on the desktop, at any size |
| Headings | 3 levels, bold, extra spacing | Same |
| Images | Pre-scaled at ingest to 480 px wide, 1-bit FS dither by default, 2-bit when "grey images" is on | 2-bit, better resampling |
| Tables | Stacked rows (label: value) up to 4 columns; wider tables become an image via the converter | Rendered as an image at page width |
| Math (MathML) | Alt text | Rasterised |
| Footnotes | Popup card | Popup card |
| Vertical CJK, RTL | Not on device initially | Converter FIXD handles anything |

## Throughput expectations

Ingest (one-time per book) on the C3: EPUB text at roughly 100 to 200 KB/s of XHTML; a 400-page novel ingests in 5 to 10 s. Image-heavy books are dominated by JPEG decode at roughly 0.3 megapixels per second. CBZ at 200 pages takes a few minutes, which is why the converter is recommended for comics and why ingest shows progress on the Drop page.
