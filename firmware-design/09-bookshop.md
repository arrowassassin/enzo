# 09 — Bookshop: a free, open library built into the device

The device has Wi-Fi, so it should be able to fill itself with books without a phone or computer. The Bookshop is a first-class Home entry that browses free and open-licence catalogs, shows covers and blurbs, and downloads straight into the library. No account, no payment, no DRM, ever. Research behind the source choices is in Appendix B5.

## 1. What "best" means here

The candidates were judged on breadth, EPUB quality, a machine-readable catalog that works without a login, covers, stability, and licence hygiene. Only three sources pass, and each is good at something different:

| Source | Breadth | Quality | Catalog access (Sept 2026) | Role in the Bookshop |
|---|---|---|---|---|
| **Project Gutenberg** | 75,000+ public-domain titles, all languages | Good EPUB3 (auto-generated), covers, "no images" variants of 100 to 500 KB | OPDS 1.x Atom feeds, no login, search and popularity sort; **XML feeds to be retired in 2027** in favour of OPDS 2.0 JSON (beta). Automated access is rate-limited by ModSecurity; the site also publishes an official offline catalog file for exactly this use. | The big shelf. Breadth, search, subjects, popular and new. |
| **Standard Ebooks** | ~1,500 titles | The best typography in public-domain ebooks, hand-edited EPUB3, compatible and KEPUB variants, 1400 × 2100 covers | Full OPDS is Patrons-Circle-only, **but the project explicitly grants feed access to qualifying open-source projects** on request. Direct EPUB downloads and the HTML catalog are free. A public New Releases Atom feed exists. | The curated shelf. "Best editions" and collections such as the Harvard Classics and the Modern Library 100. |
| **Palace Bookshelf** (DPLA, now Lyrasis) | ~19,000 librarian-curated open-access and Creative Commons titles, many modern | Mixed but curated, covers included | Public OPDS 2.0 feed documented in the Palace circulation manager; URL **unverified** from this sandbox | The modern shelf: contemporary CC fiction, nonfiction, textbooks. Fallback: Unglue.it OPDS 1.x (~100k works, mostly links to Gutenberg and OAPEN). |

Also worth a secondary place: **Wikisource** via the WS Export tool (any Wikisource text rendered to EPUB on demand, per-language "ready for export" OPDS lists), useful for languages Gutenberg covers thinly.

Excluded, with reasons: Feedbooks (shut down 2024), Internet Archive BookServer (dead endpoints; lending is DRM), ManyBooks (Cloudflare challenge, HTTP downgrade redirects), Baen (no feed since 2012), Global Grey and similar (scraping only, redistribution terms), Gutendex (hosted instance now behind a Cloudflare challenge; self-host only), DOAB/OAPEN (academic, PDF-heavy, feed returning empty), and any shadow library (copyright).

## 2. Architecture: shelves come from us, books come from the source

Talking to three live catalogs from a 380 KB device is fragile: feeds are large XML, formats are changing, and bot-blocking is a real risk. The Bookshop therefore uses a **project-hosted shelf index** for browsing and only contacts the source hosts to download the actual file.

```
GitHub Action (nightly)                                 device
 ├─ Gutenberg official offline catalog (CSV)  ─┐
 ├─ Standard Ebooks feed (granted access)     ─┼─► shelves/*.json  ──HTTPS──►  Bookshop UI
 ├─ Palace Bookshelf OPDS 2.0                 ─┤   catalog.idx (SD)            │ "Get"
 └─ hand-curated picks (contributors)         ─┘                               ▼
                                                              source host ──► SD → ingest → Library
```

- **Shelf index.** Small JSON files (10 to 50 KB each) on the project's static site: `popular.json` (Gutenberg top 100 by language, refreshed daily), `new.json` (Standard Ebooks and Palace new releases), `collections/*.json` (Harvard Classics Shelf of Fiction, Modern Library 100, "Short novels under 200 pages", "Start here" picks by contributors, seasonal lists), `subjects/*.json`. Each entry: source, id, title, author, language, year, short blurb, word count, EPUB size, cover URL, download URL, licence.
- **Offline catalog.** A compact index of the entire Gutenberg catalog plus Standard Ebooks and Palace (about 100,000 rows: title, author, language, year, popularity, source id; roughly 8 MB) downloaded to the SD card once and refreshed monthly. Search and browse work **offline**; only "Get" needs Wi-Fi. This is what makes the Bookshop feel instant on a device that cannot stream feeds: search is a binary search over a sorted prefix index on SD.
- **Live fallback.** If the project index is unreachable, the device can still browse Gutenberg's OPDS directly (streaming XML parser, 25-entry pages) and Standard Ebooks' public New Releases feed. The OPDS client already exists for user catalogs (06), so this costs little.
- **Downloads** go straight from the source host to the SD card in 8 KB chunks, then through the normal ingest pipeline. Gutenberg downloads redirect to mirrors; follow cross-host 301/302 but refuse HTTPS-to-HTTP downgrades.

## 3. Etiquette and reliability

- One descriptive User-Agent with the project URL; at most one request every 2 s to any source; shelf JSON cached for 12 h, covers cached on SD; never fetch on boot.
- Apply for Standard Ebooks' open-source feed access before the first release; until granted, the shelf index uses their public HTML and New Releases feed generated on the project side, not on devices.
- Budget the OPDS 2.0 JSON parser (serde + postcard-free streaming JSON, or a hand-rolled tokenizer) in milestone 3 so the device is ready before Gutenberg retires XML in 2027.
- TLS: a trimmed CA bundle covering Let's Encrypt (ISRG Root X1) and the roots used by gutenberg.org, standardebooks.org, dp.la, and GitHub Pages; one TLS session at a time; treat a Cloudflare challenge response as a hard failure with a clear message.
- Verify the Palace OPDS 2.0 URL from a real network before shipping the shelf; keep Unglue.it as the configured fallback.

## 4. Device UX

The Bookshop is designed like a small, opinionated bookshop, not a search engine. Screens (numbered to slot into the inventory in 07):

**35 Bookshop home** — Six shelves, each a horizontal row of three covers with a "More" cell: Start here (contributor picks), Popular this week, New editions (Standard Ebooks), Modern & Creative Commons (Palace), Collections, By subject. A Search entry at the top. Language filter in the status strip (defaults to the UI language; "All languages" available). Key hints: `◀ · Back · Open · ▶`, Up/Down move between shelves.

**36 Book page** — Cover left (152 × 228), title in serif 32 px, author, year, language, reading time estimate from word count ("about 6 h"), source and licence line ("Standard Ebooks · public domain"), then the blurb paginated. Primary action **Get** (becomes a stepped progress bar, then **Read**). Secondary: Save for later, More by this author, Same collection. If the title is already in the library the button reads **Read** from the start.

**37 Search** — Keyboard at the bottom, live results above from the offline index as you type (title and author prefix match), sorted by popularity; result rows show title, author, year, source glyph. Works with Wi-Fi off; "Get" prompts to connect.

**38 Browse** — Subjects (Gutenberg bookshelves and subjects merged with Palace categories), Authors A to Z, Collections (each a curated list page with a one-line description), Languages.

**39 Downloads & Saved** — Queue with per-item progress and errors ("Gutenberg is limiting requests, retrying in 30 s"), and the Save-for-later list with a **Get all** action for when Wi-Fi is available.

Flows to storyboard: Home → Bookshop → Popular → Book → Get → Read (5 presses to a new book); Search offline with Wi-Fi off, save three books, connect later, Get all; Book page → More by this author → Get.

## 5. Feature list and tiers

| Feature | Tier |
|---|---|
| Bookshop home with the six shelves from the project index; Get and Read; download queue; covers cached | T1 |
| Offline catalog on SD with search and browse; Save for later and Get all | T1 |
| Live OPDS fallback to Gutenberg when the index is unreachable | T1 |
| Collections and contributor "Start here" picks, editable on GitHub by contributors | T1 |
| Palace Bookshelf modern shelf (once the feed is verified) with Unglue.it fallback | T2 |
| Wikisource on-demand EPUB for thin languages | T2 |
| OPDS 2.0 JSON support ahead of Gutenberg's XML retirement | T2 (before 2027) |
| Reading-history-based suggestions computed on the device ("because you finished Austen") from subject tags in the offline index | T3 |
| Author pages with portraits and a short bio from the index | T3 |

## 6. Paid books

A seven-key device is the wrong place to type card numbers, and no DRM-free store offers a device-friendly purchase API. The design keeps purchases on the phone: buy from a DRM-free store (Standard Ebooks' partner stores, Smashwords/Draft2Digital, Leanpub, publisher direct), then send with the Drop page or the iOS Shortcut. The Bookshop stays free-only by design and says so on its first screen.
