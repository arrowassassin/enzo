# 02 — Xteink X3 hardware fact sheet

Everything a firmware author needs to know about the board, with the contradictions called out. Source keys are expanded at the bottom. Facts marked **⚠ verify** must be confirmed on a real unit before code depends on them.

## 1. Compute and memory

| Item | Value | Source |
|---|---|---|
| SoC | ESP32-C3, single-core RISC-V RV32IMC | PX, CP |
| Clock | 160 MHz active; CrossPoint and Papyrix idle at 10 MHz to save power | PX, CP |
| SRAM | 400 KB total, ~380 KB usable by the application (16 KB reserved for cache) | CP, Espressif datasheet |
| PSRAM | **None.** This single fact shapes the whole architecture. | PX, CP, Adafruit |
| Flash | 16 MB external SPI flash (DIO), ROM bootloader, app at 0x10000 | CP platformio.ini |
| Radio | Wi-Fi 4 (2.4 GHz) + BLE 5.0 (shared radio, coexistence) | ESP32-C3 datasheet |
| USB | Native USB-Serial/JTAG on GPIO18/19 (no USB OTG, so no mass-storage mode is possible) | ZOCS, CP |

The heap actually available to a Rust firmware depends on the stack: pulp-os measured ~172 KB on esp-hal 1.0 with embassy on the X4; ESP-IDF with Wi-Fi active typically leaves under 300 KB. With Wi-Fi and BLE both up, expect roughly 100 to 150 KB free. The architecture document budgets for that.

## 2. Display

| Item | Value | Source |
|---|---|---|
| Panel | 3.68 in (marketed 3.7 in), **792 × 528**, monochrome with 4-level greyscale, ~259 PPI | PX, XT |
| Panel ID in stock firmware | `ZHX368`, `LUT_VER 0x66`; no public Good Display part number found | PX-8279 |
| Controller, revision A | **UC8253** (original units). SPI max 10 MHz; 20 MHz caused visible pixel damage. Full-frame only, no window RAM commands. | PX, PX-LUT |
| Controller, revision B | **UC8279d_B** (units shipped since ~July 2026). 20 MHz SPI OK. Blank MTP, so the host must upload all registers and waveforms at init; OTP mode leaves the panel dark. | PX-8279, FI |
| Detection at boot | Reset pulse (10 ms high, 50 ms low, 50 ms high), read 3 bytes of VER (0x70) half-duplex on MOSI. Byte 2 = 0x66 → UC8279, 0xFF → UC8253. | PX-8279, FI |
| Frame size | 792 × 528 / 8 = 52,272 bytes per 1-bit plane (~42 ms transfer at 10 MHz). A 2-bit greyscale frame is two planes. | PX-LUT |
| Refresh timings (UC8253, PLL 0x09) | Full GC ≈ 472 ms; partial "turbo" DU ≈ 382 ms; image-sync ≈ 908 ms; 4-grey pass ≈ 127 ms | PX-LUT |
| UC8279d specifics | KW mode, DTM1 = old plane, DTM2 = new plane, external GC/DU waveforms, 4-grey via two planes. Init: PSR 3F 4A, PTL 792×528, PWR 43 00 78 78 17, PLL 0F. | PX-8279 |
| Contradiction | One stock-firmware reverse-engineering gist labels the X3 display "SSD1677". Every driver source disagrees. The SSD1677 is the X4's controller. | GIST vs PX/FI |

Consequences for design: there is no partial-window update on the UC8253, so every refresh sends a full 52 KB frame. The budget therefore needs one full 1-bit framebuffer (52 KB) resident in RAM, or paged rendering into a small band buffer streamed to the controller. Both controllers must be supported by the same binary via runtime detection.

## 3. Pin map

| GPIO | Function | Notes |
|---|---|---|
| 0 | I²C SCL | On the X4 this pin is the battery ADC instead |
| 1 | Button ADC ladder, group 1 | Back ≈ 3512 mV, Confirm ≈ 2694, Left ≈ 1493, Right ≈ 5 |
| 2 | Button ADC ladder, group 2 | Up ≈ 2242 mV, Down ≈ 5 |
| 3 | Power button, active-low, deep-sleep wake source | Possibly also IMU INT1 **⚠ verify** |
| 4 | EPD DC | |
| 5 | EPD RST, active-low | |
| 6 | EPD BUSY, low = busy | |
| 7 | SD MISO | |
| 8 | SPI SCLK, shared EPD + SD | |
| 9 | BOOT strap | |
| 10 | SPI MOSI, shared EPD + SD | Also used half-duplex to read the panel VER register |
| 12 | SD CS | Must be held high while probing or driving the EPD |
| 13 | Active-high SD power rail enable | Drive low and `gpio_hold` before deep sleep or the card drains the battery in about a day |
| 18 / 19 | USB D− / D+ | USB-Serial/JTAG, VID:PID 303a:1001 |
| 20 | I²C SDA | On the X4 this pin is USB-detect; on the X3 an idle-high read here falsely reports "USB present" if X4 logic runs |
| 21 | EPD CS | |

Sources: PX, FI, GIST, CP. One contradiction: CrossPoint's power manager comments GPIO13 as "gates the battery MOSFET" while FreeInk and Papyrix say it is the SD rail. Either way, the rule is the same: drive it low before sleep.

Button ladder decoding: the stock-firmware gist reports different raw thresholds and only four key codes (OK/Power, Up, Down, Back), so the mapping of physical keys to ladder positions on the X3 is **⚠ verify** on hardware with a multimeter or a debug screen that shows raw ADC readings.

## 4. I²C peripherals (400 kHz)

| Device | Address | Use |
|---|---|---|
| BQ27220 fuel gauge | 0x55 | State-of-charge register 0x2C, voltage 0x08, current 0x0C signed (positive = charging = USB present). Battery configured as 642 mAh. There is no ADC battery sense on the X3. |
| DS3231 RTC | 0x68 | Real-time clock with battery-backed time; gives a real clock for stats and sleep screens |
| QMI8658 IMU | 0x6B or 0x6A | Accelerometer and gyro. Stock firmware uses it for shake or tilt page-turn. Also enables auto-rotate. |

Board detection used by FreeInk: at least two of these devices ACK in two passes → X3.

## 5. Inputs, indicators, physical

- Buttons: **four along the bottom edge** (Left, Centre-Left, Centre-Right, Right — the two centre keys act as Back and Confirm in CrossPoint), **two on the right side** (Up, Down), a **Power** key, and a recessed reset on the underside. Seven inputs plus reset. No touch screen. One source describes the front keys as two rocker switches; the CrossPoint user guide and papyrix docs describe four discrete keys. **⚠ verify** on your unit and photograph it for the designer.
- No LEDs, no front light, no speaker, no audio output of any kind.
- NFC: a passive MIFARE Classic 1K tag, not connected to the MCU (unconfirmed). Not usable by firmware.
- Dimensions 97.6 × 63.7 × 5.1 mm, 58 g. Price about $79.
- MicroSD slot: ships with a 16 GB card. Vendor claims up to 256 GB; other listings say 512 GB. Cards above 32 GB ship formatted exFAT, which matters for the filesystem choice.

## 6. Power

- Battery 650 mAh official (642 mAh in the gauge config). Charging is over the pogo connector at 5 V; no charger IC identified yet.
- Deep-sleep current: ~10 µA claimed for the X4, X3 figure unconfirmed. The only wake source is GPIO3 (power button) going low. A page turn from deep sleep is not possible; the device must be "light asleep" (CPU idle, radio off, panel powered down) for instant page turns and only deep-sleep after a longer timeout.

## 7. Connector, flashing, recovery

- **No USB-C.** The X3 has a 4-pin magnetic pogo connector carrying 5 V, GND, D+, D− straight to the ESP32-C3 USB-Serial/JTAG. The device must be powered on before the cable attaches.
- Flash with esptool or espflash: app only at 0x10000, or bootloader at 0x0, partition table at 0x8000, app at 0x10000. Back up first with `read_flash 0 0x1000000`.
- CrossPoint partition table (a proven layout to reuse): nvs 0x9000 (20 KB), otadata 0xE000 (8 KB), ota_0 0x10000 (6.25 MB), ota_1 0x650000 (6.25 MB), spiffs 0xC90000 (3.4 MB), coredump 0xFF0000 (64 KB). Stock firmware also uses an otadata + dual-OTA layout and its bootloader is honoured.
- **USB flash lock.** Some units (AliExpress and Taobao batches, and reportedly some from xteink.com) have the "disable download mode" eFuse burned. Locked units do not even enumerate over USB. This is irreversible. The only entry points are: the stock firmware's SD update path (`update.bin` at the card root, hold left side button + Power at boot), an OTA "unlocker" image that then allows further OTA updates, or a CH341a programmer clipped to the SPI flash. Any custom firmware therefore **must** ship a robust OTA-from-SD and OTA-from-Wi-Fi path from day one, because for locked units it is the only way in and out.
- Recovery: CrossPoint's escape-hatch project uses a Back + Up combo at reset to enter a minimal recovery app. Copy this idea: a tiny always-present recovery partition that can flash from SD.

## 8. Stock firmware (for feature parity reference)

Versions seen: 5.0.3 and V6.3.15 (July 2026). Reads EPUB, TXT, JPG, BMP and `.bin` bitmap fonts. Transfers via a companion phone app over Wi-Fi/cloud, or by microSD. No USB mass storage. Users moved to CrossPoint mainly for typography, EPUB fidelity, dictionary, sync, and local (not cloud) transfer.

## 9. Known quirks checklist

1. Shared SPI bus: keep SD CS high during EPD operations and never interleave transactions.
2. UC8253 units: never exceed 10 MHz SPI.
3. UC8279d units: a driver that does not upload waveforms shows a blank screen. Detect the controller at boot.
4. Pull GPIO13 low with hold before deep sleep, or the SD card drains the battery.
5. GPIO20 is I²C SDA on the X3, not USB-detect. Use the fuel gauge's current sign to detect USB power.
6. Cable attach order: power on first, then connect the pogo cable.

## Source keys

- **PX** papyrix-reader `docs/x3-specifications.md`, <https://raw.githubusercontent.com/bigbag/papyrix-reader/main/docs/x3-specifications.md>
- **PX-LUT** papyrix-reader `docs/x3-lut-waveforms.md` (same repo)
- **PX-8279** papyrix-reader `docs/x3-uc8279-driver-reference.md` (same repo)
- **FI** FreeInk SDK `libs/hardware/BoardConfig/include/BoardConfig.h` and `docs/xteink-x3-uc8279-support.md`, <https://github.com/Free-Ink/freeink-sdk> (submodule of crosspoint-reader develop)
- **CP** crosspoint-reader develop branch: `platformio.ini`, `partitions.csv`, `lib/hal/HalPowerManager.cpp`, `lib/hal/HalGPIO.cpp`, `docs/fix-bricked-xteink.md`, <https://github.com/crosspoint-reader/crosspoint-reader>
- **GIST** stock 5.0.3 firmware reverse-engineering notes, <https://gist.github.com/CrazyCoder/1c5f846adee18e21f91e264601a6ddce>
- **ZOCS** <https://github.com/zocs/eink-quick-flasher/blob/main/X3-FLASHER-GUIDE-EN.md>
- **EH** <https://github.com/crosspoint-reader/escape-hatch>
- **XT** <https://www.xteink.com/products/xteink-x3> (product page)
- **Adafruit** <https://learn.adafruit.com/circuitpython-on-the-xteink-x4-ereader>
