# Appendix B5 — Free ebook catalog survey (agent report, 2026-09-13)

Verbatim report from the targeted catalog research pass. Method note: the sandbox's egress proxy blocked direct fetches of gutenberg.org, standardebooks.org, gutendex.com, unglue.it, archive.org, feedbooks.com, oapen.org, dp.la and wmcloud.org, so "live in 2026" status below comes from search snippets, GitHub issues and third-party 2026 catalog surveys rather than direct HTTP probes. Anything marked *(unverified)* should be probed from the device or a dev box before shipping.

## 1. Project Gutenberg — public domain, ~75,000+ titles

- **Scale/licence:** >75,000 public-domain ebooks ([UConn guide](https://guides.lib.uconn.edu/projectgutenburg)).
- **OPDS 1.x (Atom):** root `https://www.gutenberg.org/ebooks.opds/` (KOReader/Foliate ship `https://m.gutenberg.org/ebooks.opds/`, an alias for www — [KOReader plugin](https://github.com/koreader/koreader/blob/master/plugins/opds.koplugin/main.lua), [Foliate](https://github.com/johnfactotum/foliate)); search at `https://www.gutenberg.org/ebooks/search.opds/` ([PG offline catalogs page](https://www.gutenberg.org/ebooks/offline_catalogs.html)). Root feed exposes Popular (`/ebooks/search.opds/?sort_order=downloads`), Latest (`?sort_order=release_date`), Random, plus Bookshelf/Subject/LoCC browse ([KOReader test fixture](https://github.com/koreader/koreader/blob/master/spec/unit/opds_spec.lua)). Entries carry `application/epub+zip` acquisition links (images/no-images variants), Kindle links, and `opds-spec.org/image` + `image/thumbnail` JPEG covers (same fixture).
- **OPDS 2.0 (JSON):** in beta, access on request; PG "expects to sunset the existing XML-based OPDS feeds in 2027" ([offline catalogs](https://www.gutenberg.org/ebooks/offline_catalogs.html); KOReader got beta access, [PR #15696](https://github.com/koreader/koreader/pull/15696)). Plan for a JSON parser path.
- **Formats:** EPUB3 (default, smaller), EPUB2 for old readers, "EPUB no images" for size ([bibliographic record help](https://www.gutenberg.org/help/bibliographic_record.html)). Files live at `/cache/epub/{id}/pg{id}.epub` and mirrors, e.g. `https://aleph.gutenberg.org/cache/epub/{id}/pg{id}.epub` ([gutenberg-bulk-downloader](https://github.com/puntonim/gutenberg-bulk-downloader)); mirror list `https://www.gutenberg.org/MIRRORS.ALL` and rsync `gutenberg.pglaf.org::gutenberg-epub` ([mirroring how-to](https://www.gutenberg.org/help/mirroring.html)).
- **Top 100:** HTML only at `https://www.gutenberg.org/browse/scores/top` (per-language `top-en.php` etc.) ([PG](https://www.gutenberg.org/browse/scores/top)); OPDS "Popular" sort is the machine equivalent.
- **Bot policy:** site is "intended for human users only"; automated tools get temporary/permanent IP blocks; blocking is OWASP ModSecurity CRS (not Cloudflare) with rate limiting, blocks may last 24 h; use `robot/harvest` for bulk ([robot access](https://www.gutenberg.org/policy/robot_access.html), [block.pglaf.org](https://block.pglaf.org/)). Plain HTTP retired April 2021 — HTTPS only ([KOReader #7439](https://github.com/koreader/koreader/issues/7439)).
- **Gutendex JSON API** (`https://gutendex.com/books?search=…&languages=en&sort=popular`, 32/page, formats dict incl. `image/jpeg` cover, no key) ([README](https://github.com/garethbjohnson/gutendex)) — but as of Aug 2026 the hosted instance returns 403 with a Cloudflare JS challenge and its robots.txt disallows `/books/` ([api-evangelist issue](https://github.com/api-evangelist/roadmap/issues/143)). Self-host or skip.

## 2. Standard Ebooks — 1,502 titles, best typography

- Public domain (text, cover and their own work) ([about](https://standardebooks.org/about)); 1,502 books as of Aug 2026 ([Wikipedia](https://en.wikipedia.org/wiki/Standard_Ebooks)).
- **Feeds:** `https://standardebooks.org/feeds/opds`, `/opds`, `/feeds/opds/all`, `/feeds/opds/new-releases` return **401**; the full OPDS catalog is reserved for Patrons Circle, ebook producers, and qualifying open-source projects (email as username, blank password); scrapers get IPs blocked ([feeds page](https://standardebooks.org/feeds), [CrossPoint #2284](https://github.com/crosspoint-reader/crosspoint-reader/issues/2284)). Only the New Releases Atom/RSS (`https://standardebooks.org/feeds/atom/new-releases`, 15 newest, enclosure links) is public ([feeds/atom](https://standardebooks.org/feeds/atom)). Open-source projects can request access ([feeds](https://standardebooks.org/feeds)).
- **HTML + direct downloads are free, no login:** `https://standardebooks.org/ebooks?sort=popularity` and `…/ebooks/{author}/{title}/downloads/{author}_{title}.epub` ([example](https://standardebooks.org/ebooks/philip-gibbs/now-it-can-be-told/downloads/philip-gibbs_now-it-can-be-told.epub)); bulk zips are patron-only ([bulk downloads](https://standardebooks.org/bulk-downloads)).
- **Variants:** compatible EPUB (all readers except Kindle/very old), KEPUB, AZW3, "advanced" EPUB3 not for general use ([how to use](https://standardebooks.org/help/how-to-use-our-ebooks)). Covers 1400×2100 ([cover how-to](https://standardebooks.org/contribute/how-tos/how-to-choose-and-create-a-cover-image)).
- **Collections** (Harvard Classics Shelf of Fiction, Modern Library 100 Best Novels/Nonfiction, etc.) have OPDS feeds, but under the same patron gate ([collection feeds](https://standardebooks.org/collections/harvard-classics-shelf-of-fiction/feeds)).

## 3. Feedbooks — dead

Shut down 2024, domain redirects to Cantook; every old OPDS URL is dead ([Wikipedia](https://en.wikipedia.org/wiki/Feedbooks), [MobileRead](https://www.mobileread.com/forums/showthread.php?t=359541)). KOReader removed it in 2025 after "Failed to authenticate" errors ([koreader #13340](https://github.com/koreader/koreader/issues/13340)). Exclude.

## 4. Internet Archive / Open Library

- `bookserver.archive.org/catalog/` historically returned 502s on downloads ([koreader #6941](https://github.com/koreader/koreader/issues/6941)) and by Aug 2026 the catalog and domain root time out entirely ([justRead 2026 survey](https://justread.app/en/posts/best-free-opds-catalogs-2026)). Dead.
- Bulk metadata: `https://archive.org/advancedsearch.php?q=…&output=json&rows=100` (10,000-result cap) ([archive.org search help](https://archive.org/help/aboutsearch.htm)); item file lists give direct EPUB download URLs. Beware OCR-generated EPUBs are low quality.
- Open Library lending (CDL) uses Adobe DRM / ACSM and now LCP — unusable on-device ([Open Library borrow FAQ](https://openlibrary.org/help/faq/borrow)). An Open Library OPDS 2.0 endpoint exists (`/v1/api/opds`) via ArchiveLabs' Lenny ([lenny #203](https://github.com/ArchiveLabs/lenny/issues/203)) *(unverified)*.

## 5. DPLA / Palace Bookshelf

Palace Bookshelf offers >19,000 free open-access/public-domain ebooks, curated by librarians ([DPLA](https://dp.la/news/5-things-you-might-not-know-about-palace-bookshelf)); ownership moved to Lyrasis June 2025. The circulation manager README names a public OPDS 2.0 feed `https://palace-bookshelf-opds2.dp.la/v1/publications` and the older crawlable `http://openbookshelf.dp.la/lists/Open%20Bookshelf/crawlable` ([circulation README](https://github.com/ThePalaceProject/circulation)). `openbookshelf.dp.la` did not resolve in DNS from the sandbox; the OPDS2 URL is *unverified* but the most promising CC/OA aggregator with covers.

## 6. Unglue.it

OPDS root `https://unglue.it/api/opds/`, no key, 30 records/page via `page`, per-work `…/api/opds/all/?work=ID` ([API help](https://unglue.it/api/help)). ~100,000 works: 60k Gutenberg, 30k DOAB CC titles, 5k CC textbooks ([blog](https://tagteam.harvard.edu/hub_feeds/4250/feed_items/3416188/about)). Mostly links out to other hosts (Gutenberg/OAPEN), so downloads may redirect cross-domain.

## 7. DOAB / OAPEN

REST search `https://directory.doabooks.org/rest/search?query=…&expand=metadata,bitstreams` with `Accept: application/json` ([DOAB API](https://www.doabooks.org/en/article/api-search-doab)). OAPEN OPDS `https://library.oapen.org/opds` was returning 200 with zero bytes in 2026 ([justRead](https://justread.app/en/posts/best-free-opds-catalogs-2026)); all content is PDF, EPUB only for some ([St Andrews guide](https://libguides.st-andrews.ac.uk/c.php?g=720477&p=5283339)). Academic, PDF-heavy — low priority.

## 8. Wikisource (WS Export)

Tool `https://ws-export.wmcloud.org/` generates EPUB (default epub-3), PDF, txt, mobi on demand; parameters lang, title/page, format, images, fonts ([WS Export](https://wikisource.org/wiki/Wikisource:WS_Export), [repo](https://github.com/wikimedia/ws-export)). Daily-regenerated OPDS per language, e.g. `https://ws-export.wmcloud.org/opds/en/Ready_for_export.xml` ([Help:OPDS](https://en.wikisource.org/wiki/Help:OPDS)). Generation is slow (seconds) and volatile.

## 9. Smaller sites

- **ManyBooks** `https://manybooks.net/opds/index.php` — 50k titles but 302s to plain-HTTP `srv.manybooks.net` and sits behind a Cloudflare challenge ([getbookshelves](https://github.com/getbookshelves/opds-catalog), [justRead](https://justread.app/en/posts/best-free-opds-catalogs-2026)). Skip.
- **Baen Free Library** — OPDS discontinued in 2012; HTML only ([TeleRead](https://teleread.com/baen-webscriptions-is-now-baenebooks-com/index.html)). Skip.
- **Smashwords** legacy `http://www.smashwords.com/lexcycle/` — status unknown after Draft2Digital migration ([MobileRead](https://wiki.mobileread.com/wiki/SmashWords)). Skip.
- **Global Grey** (~2,000 titles, no redistribution), **Planet eBook**, **Loyal Books**, **Leanpub free** — HTML only, scraping required. Skip.

## 10. Anna's Archive / Z-Library / LibGen

Self-described pirates, subject of a 2026 thirteen-publisher lawsuit and Notorious Markets listing ([AAP](https://publishers.org/news/publishers-file-suit-against-notorious-pirate-site-annas-archive/)). Exclude.

## 11. Self-hosted URL patterns

Calibre-Web `/opds`; Kavita `/api/opds/<api-key>`; Komga `/opds/v1.2/catalog` and `/opds/v2/catalog` on port 25600 ([Komga docs](https://komga.org/docs/guides/opds/), [Kavita wiki](https://wiki.kavitareader.com/guides/features/opds/), [calibre-web #2103](https://github.com/janeczku/calibre-web/issues/2103)). CrossPoint stores up to 8 servers, HTTP Basic only ([USER_GUIDE](https://github.com/crosspoint-reader/crosspoint-reader/blob/master/USER_GUIDE.md)).

## 12–13. Curated shelves and what other readers ship

- **Gutenberg:** Popular/Latest/Random OPDS sorts, bookshelves and subjects (fixture above); Top-100 HTML.
- **Standard Ebooks:** collections (patron-gated feeds, free HTML).
- **DPLA:** Curation Corps hand-picks Palace Bookshelf titles.
- **Defaults elsewhere:** KOReader ships Gutenberg, Standard Ebooks, ManyBooks, Internet Archive, textos.info, Gallica ([main.lua](https://github.com/koreader/koreader/blob/master/plugins/opds.koplugin/main.lua)); Foliate ships Gutenberg and Standard Ebooks ([library.js](https://github.com/johnfactotum/foliate)); Thorium recommends Gutenberg, Standard Ebooks, Feedbooks, Europeana, Open Library, ManyBooks ([EDRLab](https://thorium.edrlab.org/en/th3/get_ebooks/)); Moon+ Net Library: Gutenberg, Feedbooks, ManyBooks, Smashwords ([Cafe Arcane](https://arcane.org/2021/02/21/supercharge-your-collection-with-net-libraries-to-moon-reader/)); Librera ships a preset list with "Restore default" ([Librera FAQ](https://librera.mobi/faq/working-with-opds-online-catalogs/)); CrossPoint ships none (feature request [#2494](https://github.com/crosspoint-reader/crosspoint-reader/issues/2494)). Dynamic catalog-of-catalogs: `https://opdshome.uo1.net/` ([OPDS Home](https://opdshome.uo1.net/)).

## Recommendation

1. **Project Gutenberg OPDS** — only source with breadth, covers, search, popularity sort and no login; small XML pages; EPUB3 default. Send a descriptive User-Agent with contact URL and throttle (≤1 req/2 s) to avoid the ModSecurity block. Budget a JSON OPDS 2.0 path before 2027.
2. **Standard Ebooks (HTML scrape + direct EPUB)** — best EPUB quality, tiny catalog (1.5k), but OPDS is patron-only; either apply for open-source-project feed access, or ship the public New Releases Atom feed plus a device-side static index of the 1.5k `downloads/` URLs regenerated at firmware build time. Do not hammer the site.
3. **Palace Bookshelf OPDS 2.0** (`palace-bookshelf-opds2.dp.la/v1/publications`) — 19k curated CC/OA modern titles with covers, no app needed *if* the URL is confirmed live. Fallback: unglue.it OPDS 1.x.

## Gotchas for the embedded client

- **Redirects:** Gutenberg downloads bounce to mirrors; ManyBooks and older feeds redirect HTTPS→HTTP ([koreader #7007](https://github.com/koreader/koreader/issues/7007)); follow 301/302 across hosts but refuse downgrade.
- **Cloudflare JS challenges** (gutendex.com, ManyBooks) cannot be passed by a bare TLS client — treat `cf-mitigated: challenge` as a hard failure.
- **TLS:** Let's Encrypt chains (ISRG Root X1, R11 intermediate) need the root in the bundle; enable `CONFIG_MBEDTLS_CERTIFICATE_BUNDLE` ("common" subset) and `CROSS_SIGNED_VERIFY` (+~700 B heap) ([ESP-IDF crt bundle](https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-reference/protocols/esp_crt_bundle.html), [arduino-esp32 #8626](https://github.com/espressif/arduino-esp32/issues/8626)); keep one TLS session at a time, TLS 1.2 suffices.
- **Feed size:** Gutenberg acquisition feeds are ~25 entries but each entry carries 8–10 links and a summary; parse with a streaming XML parser (expat-style) rather than buffering. Send `Accept-Encoding: identity` unless you have RAM for zlib inflate.
- **Pagination:** follow `rel="next"`; OPDS 2.0 uses `links[rel=next]` in JSON.
- **Downloads:** stream straight to SD in 4–8 KB chunks; noimages EPUBs are ~100–500 KB, illustrated ones several MB ([PG file formats](https://www.gutenberg.org/help/file_formats.html)).
- **Etiquette:** cache the root and popular feeds for hours, never poll on boot, identify the device in User-Agent.
