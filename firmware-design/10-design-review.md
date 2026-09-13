# 10 — Design review: error-proofing, hardware coverage, and the Rust decision

A second-pass review of documents 02 to 09 done before implementation starts, while the UI is being designed. Each finding says what was wrong or missing, what changed, and where. Changes have been applied to the documents named.

## A. Was Rust the right choice?

**Yes, and the runtime under it was the wrong default.** The first edition chose no_std Rust (esp-hal + esp-rtos + Embassy + esp-radio) because two hobby Rust readers for the X4 did. Reviewing the crate survey against the feature list changes the picture:

| Concern | no_std (esp-hal + esp-radio) | std Rust on ESP-IDF (esp-idf-hal / esp-idf-svc) |
|---|---|---|
| Wi-Fi driver | Same Espressif binary blob, wrapped by esp-radio 0.18 / 1.0-beta, API still churning; Wi-Fi + BLE coexistence "works to some extent" | Same blob, driven by the ESP-IDF stack that ships in every commercial ESP32 product; power-save, roaming and coexistence mature |
| HTTP server, WebSocket, mDNS, captive DNS, WebDAV | edge-net 0.15 (one maintainer), picoserve; WebDAV to be written | esp_http_server with WebSocket, mdns component, DNS server example; WebDAV still to be written |
| TLS with certificate verification | mbedtls-rs 0.2 (weeks old) | esp-tls with the built-in certificate bundle, proven on this SoC |
| OTA with rollback | esp-bootloader-esp-idf 0.6 does slot switching; rollback and anti-rollback to be written | esp_ota with rollback, app health marking, anti-rollback |
| Deep sleep GPIO wake, light sleep with automatic power management | esp-hal docs list GPIO as light-sleep only on C3 (unverified) | `esp_deep_sleep_enable_gpio_wakeup` and `esp_pm` are documented and used by CrossPoint on this exact board |
| Filesystem | embedded-sdmmc (FAT) + exfat-slim (weeks old) | FatFs via VFS, long names, `fsync`; exFAT if the IDF build enables it, else exfat-slim |
| Proof it fits in 380 KB with this feature set | None: pulp-os and TernOS do not run the network stack | CrossPoint runs the whole envelope (WebDAV, WebSocket uploads, OPDS over TLS, OTA) on Arduino, which sits *on top of* ESP-IDF with extra overhead |
| Build | `cargo` + `espflash` only | ESP-IDF toolchain via `espup`, CMake, Python; slower builds |
| Memory safety | All Rust | Rust above a C runtime; the parsers of untrusted input (ZIP, XML, JPEG, HTTP) are still Rust |

Decision, applied to 03: **the firmware binary is std Rust on ESP-IDF.** Every document, layout, UI, analytics and app crate stays `no_std + alloc` so it runs unchanged in the desktop simulator, in the WASM converter, and in a future no_std build. The no_std path remains documented as the alternative to revisit once esp-radio reaches 1.0 and the feature set is measured. What Rust buys, regardless of runtime: memory-safe parsing of hostile files, one language from firmware to converter to web tools, host-testable crates, and a contributor base that is growing while Arduino C++ contributors are aging out.

## B. Hardware capabilities that were not being used

| Capability (from 02) | Was | Now (added to 05, and 07 where visible) |
|---|---|---|
| QMI8658 IMU has hardware tap and double-tap detection | Tilt and shake page turn only | **Tap to turn**: a tap on the back of the device turns the page (T2). Needs INT1 wired to a GPIO for wake-from-light-sleep; polling works while awake. ⚠ verify INT1 wiring |
| IMU orientation | "Auto-rotate later" | Auto-rotate with hysteresis and a lock toggle (T2); "face down = sleep now" (T2) |
| ESP32-C3 deep-sleep **timer wake** (always available) | Only Power-key wake was considered | **Night jobs** (T2): wake at a chosen hour, fetch news and Bookshop shelves, sync progress and stats, refresh the offline catalog, go back to sleep. No DS3231 alarm wiring needed |
| DS3231 temperature register; e-ink waveforms are temperature dependent | Not used | Temperature-selected LUT sets (cold, normal, warm) so page turns stay clean outdoors in winter (T1). ⚠ verify against the controller's own sensor |
| BQ27220 exposes remaining capacity, current, cycle count, health, temperature | Percent only | Battery page with days-left from real consumption, cycle count and health; charge-current sign as USB detect (T1) |
| RTC fast memory (8 KB) survives deep sleep | Not used | Resume state (book, position, screen) kept in RTC memory so wake never touches the SD card before the first refresh (T0) |
| Wi-Fi radio supports ESP-NOW without an access point | Two-player games only | **Beam** (T2): send a book to a nearby Quire without any network; also the transport for two-device play |
| BLE central role (Wi-Fi is off in Read mode, so BLE fits the budget) | BLE only for provisioning | **BLE remote and keyboard** (T2): pair a page-turner clicker or a BLE keyboard; the keyboard makes notes and search fast on the device itself |
| USB-Serial/JTAG over the pogo cable | Flashing only | **Web installer** (T0): flash from Chrome/Edge with ESP Web Tools, no toolchain; **serial transfer** (T2) as a no-Wi-Fi fallback via Web Serial. Neither works on USB-locked units |
| 16 MB flash, app is 2 to 3 MB | Two 6.25 MB app slots, 3.4 MB data | Partition table rebalanced: two 4 MB app slots, a 256 KB recovery app, ~7 MB data for fonts, the sample book, a built-in compact English dictionary, and the Bookshop shelf cache (T0) |
| Panel 4-grey mode | Images and anti-aliased text | Also used for cover thumbnails in the Library grid and the sleep screen, where dithering artefacts show most (T1) |

Not usable and documented as such: the NFC tag (passive, not connected), audio (no path), USB mass storage (no OTG), front light (none).

## C. Design errors found and fixed

1. **JPEG decoding was hand-waved.** zune-jpeg decodes full frames; a 1600 × 2400 cover needs 3.8 MB. Fixed in 03: an MCU-row streaming baseline decoder with 1/2, 1/4, 1/8 DCT scaling (picojpeg or TJpgDec style, ported to Rust) is a milestone 1 deliverable; progressive JPEGs are not decoded on the device and are routed to the converter with a clear message.
2. **The screen mirror used PNG.** Deflate needs a 32 KB window on the device for nothing. Fixed in 03: frames go over the WebSocket as a 1-bit run-length format decoded in 20 lines of JavaScript.
3. **Three timings on one key.** The Peek strip needed Back to distinguish a short press, a 200 to 500 ms hold, and a long press. That is error-prone with a debounced ADC ladder. Fixed in 07: the Peek strip is removed; its information (chapter, time left, clock, battery) is already the first line of the Home layer, one short press away.
4. **Key chords were assumed freely.** The keys are two ADC ladders, so two keys in the same group cannot be read at once. Fixed in 02 and 07: chords are only allowed across groups or with Power (Power + Down for screenshot, Back + Up at reset for recovery are both valid).
5. **Light-sleep wake from the keys was unspecified.** ADC ladders cannot raise a GPIO interrupt on every key. Fixed in 03: in light sleep the keys are polled by a 25 ms timer wake (under 1 mA average), and GPIO-level wake is used only if the ladder idle level allows it on a given unit (calibration screen records this).
6. **OTA images were not signed.** A compromised release CDN could brick or take over every device. Fixed in 03 and 08: releases are signed (ed25519, minisign format); the device verifies before switching slots; the new image must mark itself healthy within 60 s or the bootloader rolls back; version anti-rollback is on.
7. **No boot-loop protection.** Fixed in 03: a crash counter in RTC memory; three consecutive crashes boot into safe mode (reader only, no apps, no Wi-Fi) with the crash log offered on the Drop page; the recovery slot remains reachable with Back + Up.
8. **SD corruption was not addressed.** FAT is not journaled and cards get pulled. Fixed in 03: every cache and index write goes to a temp file then rename; the session log is append-only with per-record checksums; every index is rebuildable from the source files; the card's volume serial is checked on mount and a changed card triggers a rescan; "Safe to remove" appears in the Power menu when no writes are pending.
9. **The memory budget double-counted the second plane.** The shadow plane for pre-render and the second plane for 4-grey are the same 52 KB and cannot be used simultaneously. Fixed in 03: a grey page is rendered without a pre-rendered successor; the turn after a grey page lays out on demand (about 100 ms extra), which is acceptable because grey pages are rare.
10. **Wi-Fi credentials in plain NVS.** Fixed in 03: NVS encryption on by default; flash encryption is left optional because it complicates the SD update path on USB-locked units.
11. **TLS session count.** OPDS, OTA and the Bookshop could open sockets concurrently. Fixed in 03: one TLS session at a time, queued.
12. **KOReader sync details.** The protocol keys progress by a document hash (partial MD5 of the file); the design now computes and stores that hash at ingest so sync works with unmodified KOReader on other devices (05).

## D. Things checked and left as they were

- Modes (Read / Transfer / Sleep) exclusive: correct and now easier under ESP-IDF, whose Wi-Fi can be fully deinitialised to free its heap.
- Ingest-once pipeline, QTX token stream, page index per profile: sound; matches what every working reader on this SoC does.
- Converter for PDF/DjVu/DOCX: still the only realistic path.
- Bookshop shelf index hosted by the project: still the right call; the device never parses large feeds.
- Two-press rule and the six signatures: all implementable; none require anything the hardware lacks.

## E. Remaining ⚠ verify list (unchanged in substance, now shorter)

1. Physical key layout and ladder voltages on the user's unit.
2. IMU INT1 wiring (tap-to-turn wake) and whether GPIO3 is shared.
3. Heap remaining in Transfer mode under ESP-IDF with one TLS session open, measured on the device with the developer menu.
4. 4-grey LUTs for both controllers, transcribed from papyrix, validated on a UC8279d unit.
5. Palace Bookshelf OPDS 2.0 URL from a real network; Standard Ebooks feed access request.

## F. Addendum after the first build (implementation start)

The runtime decision in §A was reversed at implementation time, for a reason §A did not weigh: **buildability**. The std path needs the ESP-IDF toolchain and its downloads, which the build sandbox and a plain `cargo` CI cannot fetch, and it cannot be compiled or tested here at all. The no_std path compiles on stable Rust with nothing but `rustup target add riscv32imc-unknown-none-elf`, and the first Wi-Fi-capable binary built and produced a flashable image within the hour. The consistent crate set is esp-hal 1.1.2, esp-rtos 0.3.0, esp-radio 0.18.0, esp-alloc 0.10, esp-storage 0.9, esp-bootloader-esp-idf 0.5 (esp-radio has not yet been released against esp-hal 1.2). The mitigations for §A's maturity concerns are: one TLS session at a time, Wi-Fi fully torn down outside Transfer mode, every network operation with a deadline, and the portable crates untouched by the choice so a later move to ESP-IDF changes only the device workspace.
