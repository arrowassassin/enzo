# 06 — Wireless sideloading design

Goal: getting a book from a phone or laptop onto the reader should feel like AirDrop. One tap on the reader, one scan on the phone, drop the file, done. No account, no cloud, no app required. An app or a Shortcut can make it even faster, but never mandatory.

Constraints this design works within (from 02 and the survey in Appendix B):

- The ESP32-C3 has no USB OTG, so **USB mass storage is impossible**. The pogo connector only carries a serial console. Wi-Fi is the only practical path besides pulling the SD card.
- Wi-Fi throughput ceiling on the C3 is roughly 20 Mbit/s TCP with the ESP-IDF stack; SD writes over 1-bit SPI top out near 1 MB/s. A well-built upload lands at **300 KB/s to 1 MB/s**. A badly built one (4 KB buffers, multipart parsing per chunk) is 20× slower, which is exactly what users complain about with CrossPoint.
- BLE from an iPhone realistically moves 30 to 80 KB/s and requires a native app because iOS Safari has no Web Bluetooth. BLE is therefore not a book transport.
- iOS specifics: no Web Share Target for PWAs, Files "Connect to Server" is SMB-only (WebDAV needs a third-party app), Bonjour `.local` works in Safari, and Safari is exempt from the local-network permission prompt. iOS Shortcuts can POST a file from the share sheet to a local URL.
- Android specifics: Chrome supports Web Share Target for installed PWAs, `.local` resolves natively from Android 13, Android 16+ shows a local-network permission prompt.

## 1. Transports, in priority order

| # | Transport | Who it serves | Tier |
|---|---|---|---|
| 1 | **Drop page**: device-hosted web app over Wi-Fi | Everyone, every OS, no install | T0 |
| 2 | **iOS Shortcut "Send to Reader"** and **Android PWA share target** | Share-sheet, one tap | T1 |
| 3 | **OPDS client**: device pulls from Calibre-Web, Kavita, Komga, Standard Ebooks, Gutenberg | Self-hosters, public domain readers | T1 |
| 4 | **Calibre wireless device** | Desktop Calibre users | T1 |
| 5 | **WebDAV** on the same server | Finder, Explorer, Android file managers, third-party iOS file apps | T1 |
| 6 | **SD card** | Bulk loads, recovery | T0 (always works) |
| 7 | **BLE provisioning** only: phone sends Wi-Fi credentials, device wakes its Wi-Fi | Onboarding | T2 |

Not offered: email-to-device (needs a cloud relay), AirDrop (proprietary), USB MSC (hardware).

## 2. Network modes

**Station mode (default once set up).** The reader joins the home Wi-Fi. The phone keeps its internet, the `.local` name works, and OPDS and sync can reach the LAN and the internet. Up to 8 saved networks with auto-join by signal strength.

**Hotspot mode (fallback and travel).** The reader raises an open or WPA2 SoftAP named after the device. Its captive-portal DNS answers every name with the device IP so the phone's join sheet pops up. Because the iOS captive mini-browser is stripped down and unreliable for uploads, the captive page only shows a large "Open in Safari / Chrome" link and the URL. Uploads happen in the real browser.

**Onboarding flow (first run or new network).**
1. Reader shows: "Connect me to Wi-Fi", a Wi-Fi QR (`WIFI:T:WPA;S:<name>;P:<pass>;;` for the temporary setup AP), and the URL QR.
2. Phone scans, joins, opens the setup page, picks a network from the scanned list, types the password.
3. Reader joins, shows the new `.local` name and IP, and offers "Test connection". Credentials stored in NVS.

Optional later: BLE provisioning via a companion app, and Wi-Fi credential import from a `wifi.txt` on SD (already a known trick on this hardware).

## 3. The Drop page

Served by the firmware from flash (a single compressed HTML file with inline CSS and JS, under 60 KB). Opens at `http://<name>.local` with the IP as fallback. It is a small PWA: manifest, icon, installable, and on Android it registers a `share_target` so "Share → Reader" appears in the share sheet.

Layout on the phone:
- A big drop zone / "Choose files" button at the top. Multiple files at once.
- Per-file rows: name, size, progress bar, state (queued, uploading, converting, done, failed with a reason).
- Below: the library list from the device (title, author, size, progress) with rename, delete, move to collection, download back to the phone.
- A settings tab that mirrors the device settings (read and write via a JSON API).
- A status strip: battery, free space, firmware version, "Check for update".

Upload protocol:
- Primary: `PUT /api/files/<path>` with raw body, `Content-Length`, and optional `Content-Range` for resume. The device streams the body straight into the SD file in 32 KB blocks, never buffering the file in RAM. The page splits large files into 1 MB ranged PUTs so a dropped connection resumes at the last acknowledged range.
- Fallback: `POST /upload` multipart for browsers without fetch streaming, and for the iOS Shortcut path.
- A WebSocket at `/ws` is used for live events only (progress echo to the e-ink screen, "3 books added"), not for file bytes. That keeps the file path stateless and resumable.
- Upload throughput target: at least 500 KB/s on a 2 MB EPUB, measured in the developer menu.

After upload the device runs **ingest**: identify the format, extract metadata and cover, create the library index entry, and for EPUB pre-parse the spine into the chapter cache in the background. The page shows "Ready" when ingest finishes. Unsupported formats get a clear message: "PDF is not supported on-device. Convert it in the Converter tab" with a one-tap link. DRM-protected files (encrypted EPUB/KEPUB, `.acsm` tickets) are detected at ingest and reported as "This book is DRM-protected. Remove the DRM on your computer with Calibre, then send it again"; the firmware never attempts to decrypt.

What the reader shows during transfer: a full-screen "Drop page" with the two QR codes, the URL in large text, and a live list of files arriving with a progress bar per file. Partial refresh every 1 second at most while a transfer runs, then a full refresh at the end.

## 4. Share-sheet integration without an app

**iOS Shortcut.** Ship a Shortcut (downloadable from the project site and from the Drop page) that accepts files from the share sheet, asks for nothing, and POSTs them as a form to `http://<name>.local/upload`. It shows a notification "Sent to reader". It works from Books (export via Save to Files first for non-DRM titles), Files, Safari downloads, Mail attachments. A native iOS app can come later from a contributor; the firmware API is designed so it needs nothing new.

**Android PWA.** Installing the Drop page adds it to the share sheet. Sharing an EPUB from Chrome, Drive, or a file manager opens the page with the file already queued.

## 5. Pull sources: OPDS and Calibre

**OPDS.** Saved servers (up to 8) with name, URL, optional basic auth. Browse the feed, search, page, and download straight to SD. Presets for Standard Ebooks and Project Gutenberg. HTTPS is required for public catalogs, so the firmware carries a TLS client and a trimmed CA bundle (the OPDS and OTA use cases justify the RAM).

**Calibre wireless.** Implement the smart-device protocol: listen on TCP 9090, answer the UDP discovery broadcasts on 54982, 48123, 39001, 44044, 59678, exchange JSON messages, optional password. Calibre then pushes books, updates metadata, and pulls reading position. Calibre's format preference for the device: EPUB, then our `.qbk`.

## 6. Converter

A companion **web converter** (Rust compiled to WebAssembly, hosted on the project site and also served from the device page when it fits in flash) plus a **CLI** with the same core. Both turn unsupported formats (PDF, DjVu, DOCX, RTF, CHM, AZW3, comics in CBR) into `.qbk`, the pre-laid-out page format the device streams. The converter also subsets fonts, builds font packs, pre-dithers comics to the panel size, and can batch a whole Calibre library. Design detail in 04 and 03.

## 7. Sync

- KOReader sync protocol (the CrossPoint default server and any self-hosted instance) for reading position, keyed by book hash.
- Our own `sync.json` per book on WebDAV or in the Drop page API, so two of these readers or a phone app can share position, highlights, and stats.
- Statistics export (JSON, CSV) from the Drop page.

## 8. Security defaults

- Hotspot uses WPA2 with a generated password shown on screen, not an open AP, because an open AP lets anyone nearby write to the SD card.
- In station mode the API is plain HTTP on the LAN (TLS server on the C3 costs too much RAM). An optional PIN, shown on the reader screen, gates writes when enabled.
- Wi-Fi is off unless a network feature is in use. A transfer session times out after 10 minutes idle.

## 9. Acceptance tests

1. iPhone, home Wi-Fi: scan QR from the reader, drop a 3 MB EPUB, book opens on the reader within 20 seconds of "Done".
2. Android, no home Wi-Fi: hotspot QR, join, captive page, open Chrome, install PWA, share a book from Drive.
3. Laptop: Finder connects to `http://<name>.local/webdav`, copies 20 books, library shows them without a manual rescan.
4. Calibre: device appears as a wireless device within 10 seconds, send-to-device works, position round-trips.
5. Drop Wi-Fi mid-upload, reconnect: upload resumes rather than restarting.
6. OPDS: Standard Ebooks catalog browses and downloads over HTTPS.
