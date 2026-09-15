# Quire firmware

Open-source e-reader firmware for the Xteink X3, written in Rust. The design package that this code implements lives in `../firmware-design/`; the screens follow the Claude Design batches in `../firmware-design/design/`.

## Layout

| Path | What | Builds for |
|---|---|---|
| `crates/quire-fs` | The card filesystem traits (`Fs`, `ReadAt`, `WriteFile`) every crate above the board uses. | host and device |
| `crates/quire-gfx` | 1-bit framebuffer, bitmap fonts (`.qfp` strikes), text drawing, dithering, patterns. | host and device |
| `crates/quire-fonts` | Bakes the bundled TTFs into strikes at build time: Literata 22–34 px for reading (regular, bold, italic; bold italic is the italic strike dilated), drop caps, poster and hero numerals, Atkinson Hyperlegible 18/22/26 and JetBrains Mono for the UI. | host and device |
| `crates/quire-layout` | Paragraphs into lines and pages: justification, hyphenation (hypher, seven languages), drop caps, images, footnotes, the page index. | host and device |
| `crates/quire-qtx` | The typeset text container every document is converted into. | host and device |
| `crates/quire-doc` | Document readers: EPUB/KEPUB, PDF, TXT, Markdown, FB2, HTML, CBZ, QBK. Streaming, card-backed, no DOM. | host and device |
| `crates/quire-library` | The library: scan, ingest, covers, positions, highlights, notes, stats and sessions, the news store, the Z-machine. | host and device |
| `crates/quire-ui` | Every screen, the reader, dictionaries, sleep packs, apps and games, keyboard, settings. Talks to the platform through the `Env` trait. | host and device |
| `tools/quire-sim` | The simulator: renders the real UI to PNG, drives it with scripted keys, and carries the snapshot, grammar, fuzz, persistence and efficiency tests. | host |
| `tools/quire-dict` | Builds the WordNet `.qdict` blob (`crates/quire-ui/data/en.qdict`). | host |
| `tools/quire-sleep` | Generates the sleep-image packs in `../sleep-packs/`. | host |
| `device/` | A separate workspace: the ESP32-C3 firmware `quire-x3`, the `quire-recovery` factory app, and the board (`quire-board`), panel (`quire-epd`) and network (`quire-net`) crates. | `riscv32imc-unknown-none-elf` |

## Build

Host (tests, simulator): `cargo test --workspace` (CI runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and the tests in debug mode, so integer overflow checks are on). `cargo run -p quire-sim -- --snapshots DIR` renders the 142-screen tour to PNG; `UPDATE_SNAPSHOTS=1 cargo test -p quire-sim --test screens` accepts changed hashes.

Device: `cd device && cargo build --release` builds both binaries. `cargo run --release` flashes `quire-x3` into `ota_0` over the pogo cable (the X3 must be powered on before the cable is attached); the recovery app is flashed with `espflash flash --partition-table partitions.csv --target-app-partition recovery target/riscv32imc-unknown-none-elf/release/quire-recovery`. `just images` produces the four release files; CI does the same on every push and attaches them to releases on `v*` tags.

Toolchain: stable Rust (see `rust-toolchain.toml`), no ESP-IDF, no C toolchain. The device stack is esp-hal 1.1, esp-rtos 0.3, esp-radio 0.18, embassy-net 0.9 and picoserve 0.20, the newest set of those crates that agree with each other.

## Flash layout (`device/partitions.csv`, 16 MB)

| Partition | Offset | Size | Holds |
|---|---|---|---|
| `nvs`, `otadata`, `phy_init` | `0x9000` | 36 KB | ESP-IDF bootloader data. `otadata` selects the slot; the factory image ships it pointing at `ota_0`. |
| `recovery` (factory) | `0x20000` | 512 KB | `quire-recovery` (157,216 bytes): reinstall from the card, retry, roll back. |
| `ota_0`, `ota_1` | `0xa0000`, `0x6a0000` | 6 MB each | `quire-x3` (4,171,392 bytes). Updates go to the other slot. |
| `assets` | `0xca0000` | 3.25 MB | `en.qdict`, the dictionary, read through the SPI flash driver. |
| `coredump` | `0xfe0000` | 128 KB | Reserved. |

## Memory budget

The ESP32-C3 maps at most 4 MB of flash for code and constants together (64 MMU pages of 64 KB shared by the instruction and data windows), so the firmware image has to stay under that whatever the slot size. `quire-x3` is about 1.8 MB of code and 2.0 MB of constants: 1.40 MB of font strikes (the `font_packs_stay_within_the_flash_budget` test guards 1.5 MB), the hyphenation patterns, word tables and the Wi-Fi blobs. The dictionary (1.5 MB) therefore lives in the assets partition and is read a few hundred bytes at a time.

RAM is 313 KB of DRAM plus a 64 KB region the bootloader leaves behind: a 144 KB main heap and that 64 KB region, the 52 KB panel plane, the network statics, the mirrored IRAM code of the radio, and a 40 KB main stack. On the host the UI holds about 115 KB of heap on the reading page; a Wi-Fi session (radio, stack, one HTTP connection) takes the rest, which is why the radio runs as sessions that end when the Drop page or a download is done.

## Updates and recovery

A new image is written to the other OTA slot, verified while it streams (ESP-IDF header, segments, checksum, appended SHA-256), read back and verified again, then selected as *New*. `quire-x3` marks itself *Valid* after its first frame; three crashes in a row on an unconfirmed image roll back to the previous slot. `Settings → About` offers *Install from card* when `/quire/update.bin`, `/quire-x3.bin` or `/quire-update.bin` exists, and *Restart into recovery*; holding Back for a second while powering on does the same. The recovery app shows why it booted, the two slots' versions and the card, and offers Retry, Card install and Rollback.

## Hardware notes

Values that were taken from the community's measurements of the X3 and should be confirmed on a unit before a release:

- Key ladders: GPIO1 Back 3512 mV, Confirm 2694, Left 1493, Right 5; GPIO2 Up 2242, Down 5; idle above 3850 (`quire-board/src/keys.rs`, calibrated ADC1 reads). Power is GPIO3, active low, and the deep-sleep wake pin.
- Panel: the boot-time probe tells the original UC8253 (10 MHz SPI, LUTs in registers) from the UC8279 (20 MHz) on the shared SPI bus (SCLK 8, MOSI 10, MISO 7; panel CS 21, DC 4, RST 5, BUSY 6; card CS 12, card rail GPIO13). The default rotation is portrait; *Settings → Keys → Left-handed* flips it.
- I²C on GPIO20/GPIO0 at 400 kHz: BQ27220 gauge at 0x55, DS3231 clock at 0x68, QMI8658 IMU at 0x6B or 0x6A.
- Light sleep in 30 s slices with the RTC re-read on each wake; deep sleep after the power-off timeout with the card rail held low.
