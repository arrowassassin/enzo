# Quire

Open-source e-reader firmware for the Xteink X3, written in Rust from scratch. A printed object, not a device: black ink on white paper, seven keys, the book is the home screen.

- `firmware/` — the code: portable engine crates, desktop tools and simulator, and the ESP32-C3 firmware. See [firmware/README.md](firmware/README.md).
- `firmware-design/` — the design package: research, hardware sheet, architecture, feature catalog, UX brief, and the Claude Design screen sources. Start at [firmware-design/README.md](firmware-design/README.md).
- `sleep-packs/` — fifty 1-bit sleep images in ten packs, each with a clean clock slot the device keeps live. See [sleep-packs/README.md](sleep-packs/README.md).
- `bookshop/` — the curated public-domain catalog the Bookshop shelves show.

## What it does

- **Reading.** EPUB (and KEPUB), PDF, plain text, Markdown, FB2, HTML, CBZ comics and Quire's own `.qbk`. Literata at six sizes with true bold and italic, justified and hyphenated in seven languages, drop caps, footnotes, images, a Spine progress strip, Skim, Go to, Contents, highlights and notes, and a cursor that looks any word up in the built-in WordNet dictionary (83,000 headwords) or a StarDict dictionary on the card.
- **Getting books.** Drop a card in with a `Books` folder, or open the Drop page on your phone: the reader joins your Wi-Fi (or raises its own `Quire-XXXX` hotspot) and serves a single page at `http://quire.local` for uploads, a live mirror of the screen, typing into any text field, and a JSON API. The Bookshop downloads public-domain books, OPDS catalogs work, Calibre's wireless device and KOReader progress sync are supported.
- **Apps and games.** Clock with timer and alarms, weather, news, Wikipedia, notes, flashcards, calculator, interactive fiction (Z-machine), image viewer; chess, sudoku, minesweeper, 2048 and Wordle.
- **Analytics.** Reading rhythm, calendar, goals, per-book pages and a year in review, all computed on the device from its own session log.
- **Sleep screens.** Cover, poster, quote, quick resume, blank, or an image pack with a live clock; the panel repaints the minute in light sleep for pennies of battery.
- **Recovery.** A 161 KB recovery app in the factory slot reinstalls the firmware from the card, retries or rolls back, so a failed update never bricks a unit.

## Flashing

Releases (tags `v*`) and every CI run attach four files:

| File | Flash at | What |
|---|---|---|
| `quire-x3-factory.bin` | `0x0` | Everything: bootloader, partition table, recovery app, firmware, dictionary. First install. |
| `quire-x3.bin` | partition `ota_0` | The firmware alone, for updates over Wi-Fi or from the card. |
| `quire-recovery.bin` | partition `recovery` | The factory recovery app. |
| `quire-assets.bin` | partition `assets` | The dictionary. |

First install, with the X3 powered on before the pogo cable is attached:

```sh
cargo install espflash@4.6.0 --locked
espflash write-bin 0x0 quire-x3-factory.bin
```

Updates: copy `quire-x3.bin` to the card as `/quire/update.bin` and choose *Settings → About → Install update*, or let the reader fetch the latest release itself under *Settings → Software update*. A new image boots once as *New*; the firmware confirms it after its first frame, and three crashes in a row roll back to the previous slot. Holding Back and Confirm at power-on, or *Settings → Recovery*, boots the recovery app.

## The card

FAT32, any size. Books go in `/Books` (or `/books`, or the root). Quire keeps its own files under `/.quire` and reads:

| Folder | Contents |
|---|---|
| `/Books` | Your books, in any folder structure. Covers and indexes are cached under `/.quire/books`. |
| `/sleep` | Loose `.pbm` images (528×792, 1 = ink) for the Images sleep screen. |
| `/sleep/packs/<id>/` | Sleep packs: `pack.json` plus `NN.pbm` or `NN.pbm.z`, downloaded by the reader or copied from this repository's `sleep-packs/`. |
| `/dict` | StarDict dictionaries (`.ifo`, `.idx`, `.dict`); Quire builds a small `.qix` index beside each on first use. |
| `/notes`, `/flashcards`, `/stories` | Notes and highlights export, flashcard decks, Z-machine story files. |
| `/quire/update.bin` | A firmware image to install from the card. |

## Building

Host tools, simulator and tests: `cd firmware && cargo test --workspace`. Device: `cd firmware/device && cargo build --release`, or `just images` from `firmware/` to produce the four images above. Stable Rust, no ESP-IDF, no C toolchain. See [firmware/README.md](firmware/README.md) for the crate map, the memory budget and the hardware notes.

## Licence

Quire is dual-licensed under either the [MIT licence](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option. Bundled fonts carry their own licences (SIL Open Font License) alongside the font files; the built-in dictionary is derived from WordNet 3.1 under [its licence](LICENSE-wordnet.txt); the sleep packs are CC0. Unless you explicitly state otherwise, any contribution you submit for inclusion is dual-licensed the same way, without additional terms.
