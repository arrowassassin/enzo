# Appendix B2 — Rust crate survey for an ESP32-C3 e-reader (agent report, 2026-09-13)

Verbatim report from the targeted crate survey. Versions/dates are from the crates.io API on 2026-09-13; docs.rs and docs.espressif.com were proxy-blocked, so details come from GitHub sources/READMEs (URL per row).

## 1. Platform stack

| Crate (ver, date) | no_std | Size / memory | Maint. | Verdict / source |
|---|---|---|---|---|
| esp-hal 1.2.1 (2026-09-08) | yes | stable 1.x API for esp32c3; `unstable` feature needed for ADC/sleep/etc. | Espressif-paid, very active | Use. github.com/esp-rs/esp-hal |
| esp-radio 0.18.0 stable / 1.0.0-beta.0 (2026-06-03) | yes, **alloc required** + preemptive scheduler (esp-rtos) | Quick-start heap: `64 KB` in `#[ram(reclaimed)]` + `36 KB` internal (~100 KB); community: 128 KB for Wi-Fi, ~200 KB for coex; mbedtls adds ~32 KB/connection. C3 table: Wi-Fi ✓, BLE ✓, Coex ✓ (older docs: coex "only works to some extent", avoid if possible). BLE via bt-hci 0.10 + trouble-host 0.8 | active | Wi-Fi yes; treat BLE+Wi-Fi coex as risky on 380 KB. esp-hal/esp-radio/src/lib.rs |
| esp-rtos 0.4.0 (2026-08-26) | yes | replaces esp-hal-embassy (0.9.1, 2025-10, dead); provides Embassy executor + threads for esp-radio | active | Required with esp-radio. esp-hal/esp-rtos |
| embassy-executor 0.10.0 / embassy-time 0.5.1 / embassy-sync 0.8.0 (2026-03) | yes, no alloc | task arena static | active | Use (pulp-os: 4 tasks, futures ~200 B via static cells). github.com/embassy-rs/embassy |
| esp-alloc 0.11.0 (2026-08-26) | yes | multi-region heap, `internal-heap-stats` | active | Use; put a second region in reclaimed `.dram2_uninit`. esp-hal/esp-alloc |
| esp-storage 0.10.0 / esp-bootloader-esp-idf 0.6.0 (2026-08-26) | yes | partition-table parsing, app descriptor, `ota` module (ota_0/ota_1 + otadata switching) via embedded-storage | active | OTA from Rust works; espflash's prebuilt bootloader may lack OTA → build ESP-IDF bootloader. esp-hal/esp-bootloader-esp-idf |
| esp-backtrace 0.20.0 / esp-println 0.18.0 / espflash 4.6.0 (2026-08/09) | yes | esp-println has `defmt-espflash` transport, uart/jtag-serial | active | Use. esp-rs/espflash |
| esp-idf-hal 0.46.2 / esp-idf-svc 0.52.1 / esp-idf-sys 0.37.2 (2026-03-10) | **std** (`riscv32imc-esp-espidf`) | needs ESP-IDF + cmake/python/clang toolchain; gives httpd, mDNS, Wi-Fi, NimBLE, esp-tls (TLS1.2/1.3), OTA, sleep, FATFS/SDMMC "for free"; costs FreeRTOS+lwIP+newlib static RAM (tens of KB before your app) and ~0.7–1 MB Wi-Fi binaries (approx.) | community, "little to no paid time" | Viable but heavier; **pulp-os and TernOS both chose no_std esp-hal + esp-rtos + Embassy** (TernOS uses esp-hal git for GPIO12-17). github.com/esp-rs/esp-idf-svc, hansmrtn/pulp-os, azw413/TernOS |

pulp-os budget (README): ~172 KB heap (108 KB + 64 KB reclaimed), ~56 KB stack, 4 KB display strip buffer, large statics in `StaticCell`.

## 2. Networking (no_std)

| Crate | no_std | Memory | Maint. | Verdict / source |
|---|---|---|---|---|
| embassy-net 0.9.1 (2026-04) | no alloc | `StackResources<N>` + user socket buffers; features tcp/udp/dns/dhcpv4/mdns | active | Use. embassy-rs/embassy |
| smoltcp 0.14.0 (2026-08) | no heap | code tens of KB; RAM = your socket buffers (2×~4 KB/TCP) | active | Backend of embassy-net. smoltcp-rs/smoltcp |
| edge-net 0.15.0: edge-http/-mdns/-dhcp/-captive/-ws 0.8.0 (2026-06) | no alloc | HTTP server future 6–9 KB + 4×2 KB buffers; edge-nal-embassy adapter | active (ivmarkov) | Best all-in-one: mDNS responder, captive portal, WS. github.com/sysgrok/edge-net |
| picoserve 0.20.0 (2026-08-30) | no heap | axum-like; `embassy`, `ws` features | active | Nicer router API than edge-http; either works. github.com/sammhicks/picoserve |
| reqwless 0.14.0 (2026-01) | no alloc | TLS via embedded-tls (1.3 only, **no cert verification in no_std**) or mbedtls-rs | active | HTTP client for OPDS/GitHub. github.com/drogue-iot/reqwless |
| embedded-tls 0.19.0 (2026-06) | no alloc | 16 KB record buffer; ECDSA/ed25519 stack-only, RSA needs `alloc` | embassy-rs | OK for unverified TLS 1.3. github.com/embassy-rs/embedded-tls |
| mbedtls-rs 0.2.0 (2026-08-20, ex-esp-mbedtls) | no_std + alloc | ~32 KB heap/connection default (tunable), prebuilt for riscv32imc | active | Only option for verified HTTPS + TLS1.2; budget the heap. github.com/esp-rs/mbedtls-rs |
| embedded-websocket 0.9.5 (2026-05) | no alloc | any buffer size | active | Alternative WS framing. ninjasource/embedded-websocket |

## 3. Storage

| Crate | no_std | Notes | Verdict / source |
|---|---|---|---|
| embedded-sdmmc 0.10.0 (2026-07-24) | no alloc | FAT16/32; LFN read (`iterate_dir_lfn`, `open_long_name_file_in_dir`); blocking; const-generic open-file limits | Use (pulp-os uses an async fork). rust-embedded-community/embedded-sdmmc-rs |
| embedded-fatfs (git; crates.io 0.0.0 placeholder) + sdspi + block-device-adapters 0.2.0 | yes, `lfn`, `alloc` opt | async FAT, MabezDev | Good async alternative, but git-only. github.com/MabezDev/embedded-fatfs |
| **exfat-slim 0.7.0 (2026-08-29)** | yes, alloc optional | read/write, async+blocking, block caching, Embassy example; brand-new (297 dl) | **Key finding: a usable no_std exFAT crate exists.** github.com/ninjasource/exfat-slim |
| embedded-exfat 0.4.0 (2025-11) | no_std + alloc | async, spin locks | Backup option. github.com/qiuchengxuan/exfat |
| exfat 0.1.0 (2023) | std default | stale | Skip. |
| sequential-storage 8.0.1 (2026-07) | no alloc | map/queue on NorFlash, wear-aware | Use for settings/bookmarks. tweedegolf/sequential-storage |
| ekv 1.0.0 (2024-11) | no alloc | LSM KV, tunable chunk RAM | Fine but quieter. embassy-rs/ekv |
| esp-nvs 0.5.0 (2026-07) | yes | ESP-IDF-compatible NVS bare-metal | Nice if sharing NVS with IDF tools. |
| postcard 1.1.3 | yes (heapless/alloc) | serde no_std | Use. jamesmunns/postcard |
| littlefs2 0.8.1 (2026-08) | yes | wraps C littlefs 2.9.3 → needs C cross-compiler | Only if you want a real FS in internal flash. |

## 4. Archive / compression

| Crate | no_std | Notes | Verdict |
|---|---|---|---|
| miniz_oxide 0.9.1 (2026-03) | yes; `with-alloc` optional | streaming inflate; ~11 KB state + 32 KB LZ77 window | Use for deflate. Frommi/miniz_oxide |
| **rawzip 0.5.1 (2026-07)** | yes (`alloc` feature, `from_slice` zero-copy) | zero-dependency, bring-your-own inflate; streaming reader is std | Best EPUB/CBZ base: parse central directory into a small buffer, inflate entries with miniz_oxide. nickbabcock/rawzip |
| rc-zip 5.4.1 (2025-11) | std (sans-io but no std feature) | | Skip. |
| zip 8.6.0 | std::io | | Skip. zip-lite: not on crates.io. |
| lz4_flex 0.14.0 | block format only | | Optional for cache files. |
| flate2 | std | | No. |

## 5. Markup / document formats

| Crate | no_std | Verdict |
|---|---|---|
| quick-xml 0.42.0 (2026-08) | **std-only** (no `std` feature, std::io) | No. tafia/quick-xml |
| roxmltree 0.21.1 (2025-10) | `#![no_std]` + alloc | DOM in RAM (~2–4× file) → OK per-chapter (10–100 KB). RazrFalcon/roxmltree |
| xmlparser 0.13.6 (2023) | no_std, zero-alloc, ~30 KiB code | Best streaming XHTML tokenizer; stable. RazrFalcon/xmlparser |
| html5ever 0.40.0 / lol_html 3.0.1 / kuchiki (2020, dead) | std | No. |
| pulldown-cmark 0.13.4 | no_std + alloc (`default-features=false`, hashbrown) | Use for Markdown. |
| markdown 1.0.0 | `#![no_std]` + alloc, full AST | Heavier; pulldown preferred. |
| mobi 0.8.0 (2022-12) | std (std::io/fs, whole file in RAM) | Port PalmDOC/HUFF-CDIC (~1–2 K LOC) yourself. vv9k/mobi-rs |
| fb2 0.4.4 (2023) | std (serde+quick-xml) | FB2 is plain XML → roxmltree/xmlparser. |
| pdf 0.10.0, lopdf 0.45.0 (std, no rasterizer), pdfium-render, mupdf (C++, MBs) | — | **None realistic on this MCU.** Pre-render off-device. |
| djvu 0.1.0 (2026-03) | std, full-page RGBA render, self-described "AI slop" | No. |
| docx-rs 0.4.22 (writer-focused), rtf-parser 0.4.3 | std | No. |
| unrar 0.5.8 | C++ libunrar | CBR no; CBZ yes via rawzip. |

## 6. Text

| Crate | no_std | Verdict |
|---|---|---|
| ttf-parser 0.25.1 | zero-alloc | Use. harfbuzz/ttf-parser |
| rustybuzz 0.20.1 | **archived** → harfrust 0.13.3 (2026-08; no_std via `libm`, alloc, read-fonts) | Only if you need Arabic/Indic; several hundred KB flash. |
| fontdue 0.9.4 | no_std + alloc, fully parses font to heap | pulp-os uses it at **build time** to bake bitmaps — recommended pattern. |
| ab_glyph 0.2.32 | no_std with `libm` | Lightest runtime TTF rasterizer. |
| swash 0.2.10 / cosmic-text 0.19.0 | no_std via libm; cosmic no_std lacks font loading/rendering | Too heavy. |
| embedded-graphics 0.8.2 + embedded-text 0.7.3 | no alloc | Word wrap, justify, **soft-hyphen** aware. Use. |
| u8g2-fonts 0.8.0 (2026-05) | no_std, fonts in flash | Cyrillic/CJK bitmap subsets; only linked fonts count. |
| mono_font / eg-font-converter (BDF) | no alloc | Fine for UI chrome. |
| **hypher 0.1.7 (2026-04)** | no_std, no deps, patterns embedded (English 27 KiB, German 201 KiB), alloc optional | **Use.** typst/hypher |
| hyphenation 0.8.4 (2021) | std, bincode dictionaries loaded at runtime (2.8 MB all) | No. |
| unicode-segmentation 1.13.3 / unicode-linebreak 0.1.5 (UAX14, `#![no_std]`) / unicode-bidi 0.3.18 (no_std+alloc) | yes | Use all three. |

CJK: feasible with bitmap fonts (u8g2 wqy) or subsetted TTF via ab_glyph; Arabic needs harfrust + unicode-bidi — feasible on 16 MB flash, RAM moderate, but a big scope item.

## 7. Images

| Crate | no_std | Verdict |
|---|---|---|
| zune-jpeg 0.5.15 (2026-09) | no_std + alloc | Full-frame decode (800×480 gray = 384 KB) → only with downscaled/cropped inputs; no 1/8 DCT scaling. |
| jpeg-decoder 0.3.2 | std | No. |
| png 0.18.1, lodepng 3.12.2 | std | No. |
| zune-png 0.5.2 | no_std + alloc | Same full-frame caveat. |
| minipng 1.0.0 (2025-09) | no_std, **no alloc**, caller buffer, no Adam7 | Good for small covers/icons. pommicket/minipng |
| qoi 0.4.1 / tinyqoi 0.2.0 / tinybmp 0.7.0 | no_std | Use QOI/BMP as pre-converted on-card format. |
| dither 1.3.10 (2021) | std, image crate | Hand-roll Floyd–Steinberg (2-row error buffer) / 4×4 Bayer for 1-/2-bit. |

Practical path: convert images off-device (TernOS does) or port picojpeg-style 1/8-scale decode.

## 8. E-paper

| Crate | Notes | Verdict |
|---|---|---|
| **epdsi 0.4.0 (2026-09-13)** | no_std, eh-1.0, SSD1677 (**GDEQ0426T82 800×480 = Xteink X4 panel**), UC8253, SSD168x, JD7966x; GxEPD2-style paged rendering with tiny stack buffers; DrawTarget; blocking/async; mono only | Best fit; no 4-gray LUT yet. github.com/melastmohican/epdsi |
| ssd1677 0.1.0 (h0rv, 2026-02) | no_std, custom LUT hook, full frame buffers (48 KB/plane), alloc optional | Use its LUT path for grayscale. github.com/h0rv/ssd1677 |
| ssd1677-driver 0.1.0 (codeberg sebgab, 2026-02) | no_std, e-g | Alternative. |
| epd-waveshare 0.6.0 (2024-10) | full framebuffer, no SSD1677 | Skip. |
| uc8151 0.2.0 (2023) / it8951 0.5.1 (Gray4, alloc) | other panels | N/A. |
| 4-level grayscale LUT | No crate ships it; transcribe LUTs from ESPHome PR #19213 / papyrix-reader `docs/ssd1677-driver.md` into ssd1677's custom-LUT API (TernOS shows 4-gray images on X4). |

## 9. UI toolkits

| Crate | Verdict |
|---|---|
| kolibri-embedded-gui 0.1.0 (2025-02) | egui-like immediate mode, heapless, small widget buffer; early. Usable for menus. |
| embedded-menu 0.7.0 (2026-08) / embedded-layout 0.4.2 | no_std no alloc; ideal for settings/menus. |
| slint 1.17.1 | no_std+alloc, line renderer (runs on 264 KB RP2040) but animation-oriented, 100+ KB RAM, big flash — poor e-ink fit. |
| lvgl 0.6.2 (2023-04) | unmaintained C bindings | No. |
| **Verdict**: embedded-graphics + embedded-text + embedded-menu with hand-rolled screens (what pulp-os/TernOS do). |

## 10. Misc

| Item | Finding |
|---|---|
| heapless 0.9.3, defmt 1.1.1 (+esp-println `defmt-espflash`), embassy-time 0.5.1 | all no_std, active. |
| esp-hal sleep | `LowPower::sleep_deep(RtcSleepConfig)`, timer deadline wake; module docs list GPIO as light-sleep only — verify RTC-IO deep-sleep wake on C3 (GPIO0-5) before relying on it. esp-hal/src/rtc_cntl/sleep |
| Battery ADC | `analog::adc` ADC1, `Attenuation::_11dB`, calibration schemes on C3, oneshot + async. |
| chrono 0.4.45 (`alloc` feature) / time 0.3.55 (`#![no_std]`) | either; time is lighter. |
| stardict 0.2.3 (GPL-2, std, flate2/rusqlite) / dict (2018 toy) | unusable; .ifo/.idx/.dict is trivial to parse yourself; `.dz` needs random-access gzip via miniz_oxide. |
| Chess: cozy-chess 0.3.4 (no_std, movegen only), shakmaty 0.30.1 (no_std+alloc, no engine), chess 3.2.0 (std, 2021) | cozy-chess + tiny alpha-beta. |
| sudoku 0.8.0 (AGPL-3, std, rand) | hand-roll backtracking solver. |

## Recommended stack

esp-hal 1.2 + esp-rtos 0.4 + Embassy 0.10 + esp-alloc (~100–130 KB heap incl. reclaimed DRAM2), esp-radio (Wi-Fi only; BLE off unless proven), embassy-net + edge-net (edge-http/mdns/captive/ws) or picoserve for the upload server, reqwless + mbedtls-rs for verified HTTPS (or embedded-tls if unverified is acceptable), esp-bootloader-esp-idf + esp-storage for OTA, sequential-storage + postcard for settings. Storage: embedded-sdmmc (FAT) with exfat-slim behind a volume-type switch. Documents: rawzip + miniz_oxide streaming → xmlparser/roxmltree per chapter; pulldown-cmark for MD; MOBI via a small in-house PalmDOC port; PDF/DjVu/DOCX pre-converted off-device. Text: build-time fontdue bitmaps (or ab_glyph at runtime) rendered through embedded-graphics/embedded-text, hypher for hyphenation, unicode-linebreak/segmentation. Display: epdsi (paged) or ssd1677 (custom LUT for 4-gray), hand-rolled Floyd–Steinberg. UI: embedded-graphics + embedded-menu/embedded-layout.
