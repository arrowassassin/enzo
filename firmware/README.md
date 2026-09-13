# Quire firmware

Open-source e-reader firmware for the Xteink X3, written in Rust. The design package that this code implements lives in `../firmware-design/`; the screens follow the Claude Design batches in `../firmware-design/design/`.

## Layout

| Path | What | Builds for |
|---|---|---|
| `crates/` | Portable engine: framebuffer and dithering, fonts, layout, document parsers, library, analytics, UI, apps, games. `no_std + alloc`. | host tests and the device |
| `tools/` | Desktop tools: the simulator (renders the real UI to PNG and drives it with scripted keys), the font pack builder, the sleep-image generator, the `.qbk` converter. | host |
| `device/` | The firmware binary and the board layer (ESP32-C3, esp-hal). A separate Cargo workspace so host tests never touch it. | `riscv32imc-unknown-none-elf` |
| `device/partitions.csv` | Flash layout: recovery factory slot, two 4 MB OTA slots, 7 MB assets, coredump. | |

## Build

Host (tests, simulator): `cd firmware && cargo test --workspace`.

Device: `cd firmware/device && cargo build --release`. Flash with `cargo run --release` (uses `espflash`; the X3 must be powered on before the pogo cable is attached). CI produces `quire-x3.bin` (OTA and SD update) and `quire-x3-factory.bin` (complete flash image, write at 0x0) on every push, and attaches them to releases on `v*` tags.

Toolchain: stable Rust (see `rust-toolchain.toml`), no ESP-IDF, no C toolchain. The device stack is esp-hal 1.1 + esp-rtos 0.3 + esp-radio 0.18 + Embassy, the newest set of those crates that agree with each other.
