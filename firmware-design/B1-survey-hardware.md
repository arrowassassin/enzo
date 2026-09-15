# Appendix B1 — Xteink X3 hardware survey (agent report, 2026-09-13)

Verbatim report from the targeted hardware research pass. Sources abbreviated: **[PX]** papyrix `docs/x3-specifications.md` (https://raw.githubusercontent.com/bigbag/papyrix-reader/main/docs/x3-specifications.md), **[PX-LUT]** `docs/x3-lut-waveforms.md`, **[PX-8279]** `docs/x3-uc8279-driver-reference.md` (same repo/path), **[FI]** FreeInk SDK `libs/hardware/BoardConfig/include/BoardConfig.h` + `docs/xteink-x3-uc8279-support.md` (https://github.com/Free-Ink/freeink-sdk, submodule of crosspoint-reader develop), **[CP]** crosspoint-reader develop (`platformio.ini`, `partitions.csv`, `lib/hal/HalPowerManager.cpp`, `lib/hal/HalGPIO.cpp`, `docs/fix-bricked-xteink.md`), **[SF]** https://raw.githubusercontent.com/emezrahi/XTeink-X3-Supafast/main/docs/device-specifications.md (note: this doc is actually an X4 spec), **[GIST]** https://gist.github.com/CrazyCoder/1c5f846adee18e21f91e264601a6ddce (stock 5.0.3 firmware RE), **[ZOCS]** https://github.com/zocs/eink-quick-flasher/blob/main/X3-FLASHER-GUIDE-EN.md, **[EH]** https://github.com/crosspoint-reader/escape-hatch, **[PI]** pocketink.io (faq, flash/sd-card, locked-buying-guide — egress-blocked, facts from search snippets), **[XT]** https://www.xteink.com/products/xteink-x3 (blocked; snippets), **[NBC]** notebookcheck / liliputing (snippets). juyue.tw and learn.adafruit.com were egress-blocked; only search snippets used.

## SoC / memory
- ESP32-C3 (RISC-V RV32IMC, single core), 160 MHz active; CrossPoint/Papyrix idle at 10 MHz. 400 KB SRAM (~380 KB usable), **no PSRAM**, 16 MB external SPI flash (DIO), ROM bootloader + app at 0x10000. [PX device-specifications, SF, CP platformio.ini]

## Panel
- 3.68" (marketed 3.7"), **792×528**, 4-level greyscale, ~259 PPI (sqrt(792²+528²)/3.68 = 258.7). [PX, XT]
- Panel descriptor in stock fw: `ZHX368`, `LUT_VER 0x66` — no public Good Display part number found (unconfirmed vendor). [PX-8279]
- Two controller revisions on identical PCB/glass: **UC8253** (original; 10 MHz SPI max — 20 MHz "caused pixel damage"), **UC8279d_B** (since ~July 2026; 20 MHz; blank MTP so host must upload all registers + waveforms; OTP mode leaves panel dark). Detect at boot: reset 10ms H/50ms L/50ms H, read 3 bytes of VER (0x70) half-duplex on MOSI; byte2 = 0x66 → UC8279, 0xFF → UC8253. [PX, PX-8279, FI]
- **Contradiction:** [GIST] labels the X3 display "SSD1677" — wrong per all driver sources.
- UC8253 LUTs: five 42-byte registers (0x20–0x24), PLL 0x09 ≈ 18.2 ms/frame group; full ≈ 472 ms, "turbo" partial DU ≈ 382 ms, image sync ≈ 908 ms, 4-grey ≈ 127 ms; frame transfer 52,272 B ≈ 42 ms @10 MHz. No window-RAM commands (full-frame only). [PX-LUT]
- UC8279d: KW mode, DTM1 old / DTM2 new planes, GC/DU external waveforms, 4-grey via two planes; init script PSR 3F 4A, PTL 792×528, PWR 43 00 78 78 17, PLL 0F. [PX-8279]

## Pin map (all sources agree unless noted)
| GPIO | Function |
|---|---|
| 0 | I²C SCL (X4: battery ADC) |
| 1, 2 | Button ADC ladders (group1: Back/Confirm/Left/Right; group2: Up/Down) |
| 3 | Power button, active-LOW, deep-sleep wake (`esp_deep_sleep_enable_gpio_wakeup`); possibly also QMI8658 INT1 (unconfirmed) |
| 4/5/6 | EPD DC / RST (active-low) / BUSY (LOW = busy) |
| 7 | SD MISO |
| 8 / 10 | Shared SPI SCLK / MOSI (EPD + SD) |
| 9 | BOOT strap |
| 12 | SD CS (must be HIGH during EPD probe) |
| 13 | Active-HIGH SD rail enable; driven LOW + `gpio_hold` in deep sleep |
| 18/19 | Native USB D−/D+ (USB-Serial/JTAG) |
| 20 | I²C SDA (X4: USB-detect) |
| 21 | EPD CS |
[PX, FI, GIST, CP]
- **Contradiction:** CrossPoint `HalPowerManager.cpp` comments GPIO13 "gates the battery MOSFET" on both C3 boards; FreeInk/Papyrix say X3 SD rail (confirmed via stock-fw RE). Either way: drive LOW before sleep or the card drains the battery.
- Ladder mV (shared with X4): Back ~3512, Confirm ~2694, Left ~1493, Right ~5 (GPIO1); Up ~2242, Down ~5 (GPIO2); ranges 3100/2090/750 and 1120. [FI InputManager.cpp, SF] [GIST] gives different raw thresholds and 4 codes (OK/Power, UP, DOWN, BACK) from stock fw — physical-key→ladder mapping on X3 unconfirmed.

## I²C peripherals (400 kHz)
BQ27220 fuel gauge 0x55 (SOC reg 0x2C, V 0x08, I 0x0C signed — positive = charging = USB present; battery config 642 mAh), DS3231 RTC 0x68, QMI8658 IMU 0x6B/0x6A (tilt/shake page turn). No charger IC identified; no ADC battery sense. Board detection = ≥2 of these ACK in two passes. [PX, FI, GIST]

## Buttons / inputs
Front keys plus one side key each side (per one source) or four bottom keys plus two right-side keys (per CrossPoint user guide and papyrix), power on top, recessed reset on the underside; no touch. Gyro shake-to-turn. No LEDs, no front light. [FI NO_LEDS/NO_FRONTLIGHT, NBC, CP USER_GUIDE]

## Power / USB / physical
- 650 mAh (official) [XT]; 642 mAh in gauge config [GIST]; goodereader once said 350 mAh (likely wrong).
- **No USB-C**: 4-pin magnetic pogo connector carrying 5V/GND/D+/D− → ESP32-C3 USB-Serial/JTAG (VID 303a:1001 CDC). Device must be powered on before cable attaches. [ZOCS, CP issue #2029]
- 97.6×63.7×5.1 mm, 58 g, $79; microSD (ships 16 GB; 256 GB per XT, 512 GB per other listings). [NBC, XT]
- Deep sleep ~10 µA claimed for X4 [SF]; X3 figure unconfirmed. Wake source: GPIO3 low only.

## Flashing / bootloader
- `esptool --chip esp32c3 -p /dev/ttyACM0 -b 921600 write_flash 0x10000 firmware.bin` (app only) or `0x0 bootloader.bin 0x8000 partitions.bin 0x10000 firmware.bin`; backup `read_flash 0 0x1000000`. [CP README, ZOCS]
- CrossPoint `partitions.csv`: nvs 0x9000/0x5000, otadata 0xE000/0x2000, ota_0 0x10000/0x640000, ota_1 0x650000/0x640000, spiffs 0xC90000/0x360000, coredump 0xFF0000/0x10000. Build env `esp32-c3-devkitm-1`, `ARDUINO_USB_MODE=1`, `CDC_ON_BOOT=1`, `-DFREEINK_DEVICE_X3=1 -DFREEINK_DEVICE_X4=1` (one binary). [CP]
- Stock layout: otadata + dual OTA slots, stock bootloader honoured; [ZOCS] claims stock app at 0x780000 (unconfirmed). Escape-hatch recovery combo Back+Up at reset. [EH]
- **USB lock**: some units (AliExpress/Taobao, and reportedly xteink.com) have a "disable download mode" eFuse burned; locked units don't even USB-enumerate (`error -32`). Irreversible; exact eFuse name unconfirmed. Workarounds: stock-fw SD update (`update.bin` at card root, hold **left side button + Power** at boot), OTA "Xteink Unlocker" (crosspointreader.com/#unlock-tool, safe only for CrossPoint/CrossInk), or CH341a on the SPI flash. [PI, CP issue #2029, CP docs/fix-bricked, CP README]

## Stock firmware
Versions seen: 5.0.3, V6.3.15 (July 2026). Formats: EPUB, TXT, JPG, BMP, `.bin` bitmap fonts; WiFi/cloud transfer via companion phone app or microSD; no USB mass storage; NFC is a passive MIFARE Classic 1K tag (not MCU-connected, unconfirmed). [FI docs, GIST, tabletsage/ereadersforum snippets, CP discussion #2259]

## Quirks
Shared SPI bus (keep SD CS high during EPD ops); GPIO20 idle-high can falsely read as "USB present" if X4 logic runs on X3; UC8253 driver must never exceed 10 MHz; flashing an old build on a UC8279d unit gives a blank screen; SD rail left on → 1-day battery drain. [PX, CP releases v1.5.0]
