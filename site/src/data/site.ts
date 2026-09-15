/** Every number here is taken from the repository; nothing is estimated. */

export const REPO = 'https://github.com/arrowassassin/quire'
export const RELEASES_URL = `${REPO}/releases`
export const RELEASES_API = 'https://api.github.com/repos/arrowassassin/quire/releases/latest'
export const ACTIONS_URL = `${REPO}/actions/workflows/ci.yml?query=branch%3Amain+is%3Asuccess`
export const VERSION = '0.1.0'

/** The X3 carries 16 MB of flash; a whole-flash image is exactly this long. */
export const FLASH_BYTES = 16_777_216

/** The complete flash image, written at 0x0. Also the asset name in a release. */
export const FACTORY_IMAGE = 'quire-x3-factory.bin'

/** The espflash version this project is tested against, and where to get it. */
export const ESPFLASH_VERSION = '4.6.0'
export const ESPFLASH_RELEASE_URL = `https://github.com/esp-rs/espflash/releases/tag/v${ESPFLASH_VERSION}`

export interface NavLink {
  to: string
  label: string
}

export const NAV: readonly NavLink[] = [
  { to: '/features', label: 'Features' },
  { to: '/screens', label: 'Screens' },
  { to: '/install', label: 'Install' },
  { to: '/guide', label: 'Guide' },
  { to: '/downloads', label: 'Downloads' },
  { to: '/faq', label: 'FAQ' },
]

export interface Spec {
  value: string
  label: string
}

export const SPECS: readonly Spec[] = [
  { value: '528 × 792', label: '1-bit e-ink' },
  { value: 'ESP32-C3', label: 'RISC-V @ 160 MHz' },
  { value: '16 MB', label: 'flash' },
  { value: '7 keys', label: 'and nothing else' },
  { value: 'microSD', label: 'FAT32, any size' },
  { value: 'Wi-Fi', label: '2.4 GHz, on demand' },
]

export interface ReleaseFile {
  name: string
  flashAt: string
  bytes: number
  what: string
}

export const RELEASE_FILES: readonly ReleaseFile[] = [
  {
    name: 'quire-x3-factory.bin',
    flashAt: '0x0',
    bytes: 16_777_216,
    what: 'The complete flash image: bootloader, partition table, recovery app, firmware, dictionary and otadata. This is the first install.',
  },
  {
    name: 'quire-x3.bin',
    flashAt: 'partition ota_0',
    bytes: 4_171_392,
    what: 'The firmware on its own, for updates over Wi-Fi or from the card.',
  },
  {
    name: 'quire-recovery.bin',
    flashAt: 'partition recovery',
    bytes: 157_216,
    what: 'The factory recovery app: reinstall from the card, retry, roll back.',
  },
  {
    name: 'quire-assets.bin',
    flashAt: 'partition assets',
    bytes: 1_548_664,
    what: 'The WordNet dictionary blob that lives in the assets partition.',
  },
]

export interface Partition {
  name: string
  offset: string
  size: string
  holds: string
}

export const PARTITIONS: readonly Partition[] = [
  { name: 'nvs / otadata / phy_init', offset: '0x9000', size: '36 KB', holds: 'Bootloader data; otadata selects the slot.' },
  { name: 'recovery (factory)', offset: '0x20000', size: '512 KB', holds: 'quire-recovery.' },
  { name: 'ota_0', offset: '0xa0000', size: '6 MB', holds: 'quire-x3.' },
  { name: 'ota_1', offset: '0x6a0000', size: '6 MB', holds: 'The other slot.' },
  { name: 'assets', offset: '0xca0000', size: '3.25 MB', holds: 'en.qdict, the dictionary.' },
  { name: 'coredump', offset: '0xfe0000', size: '128 KB', holds: 'Reserved.' },
]

export interface CardPath {
  path: string
  holds: string
}

export const CARD_PATHS: readonly CardPath[] = [
  { path: '/Books', holds: 'Your books, in any folder structure (/books and the card root work too).' },
  { path: '/.quire', holds: 'Quire’s own state: library index, positions, settings, stats, cover cache.' },
  { path: '/sleep', holds: 'Loose .pbm sleep images, 528 × 792, where 1 is ink.' },
  { path: '/sleep/packs/<id>/', holds: 'Sleep packs: pack.json plus NN.pbm or NN.pbm.z.' },
  { path: '/dict', holds: 'StarDict dictionaries (.ifo, .idx, .dict).' },
  { path: '/notes, /flashcards, /stories', holds: 'Notes export, flashcard decks, Z-machine story files.' },
  { path: '/quire/update.bin', holds: 'A firmware image to install from the card.' },
]

export interface Device {
  device: string
  status: 'supported' | 'not-yet'
  note: string
}

export const DEVICES: readonly Device[] = [
  {
    device: 'Xteink X3',
    status: 'supported',
    note: 'The device Quire is written for: ESP32-C3, 16 MB flash, 528 × 792 panel, either of the two controller variants (UC8253 and UC8279) probed at boot.',
  },
  {
    device: 'Other ESP32-C3 e-readers',
    status: 'not-yet',
    note: 'Not supported. A port needs a board crate for the key ladders and the I²C peripherals, a panel driver for the controller, a partition table that fits the flash, and font strikes rebuilt for the panel size — the engine crates above the board are already portable.',
  },
]

export interface Faq {
  q: string
  a: string
}

export const FAQ: readonly Faq[] = [
  {
    q: 'Is this made by Xteink?',
    a: 'No. Quire is an independent open-source project and is not affiliated with, endorsed by, or supported by Xteink. Installing it replaces the firmware your reader shipped with.',
  },
  {
    q: 'Can I go back to the stock firmware?',
    a: 'Only if you took a backup first. Reading the whole 16 MB flash to a file before you flash anything is the only way back to the factory firmware — the stock image is not published anywhere. The browser installer does the backup, and the restore, with one button each; the Guide also has the exact commands, for both espflash and esptool.',
  },
  {
    q: 'Which version is current?',
    a: 'The workspace version is 0.1.0. No tags have been published yet, so there is no release to download from the Releases page at the moment; the newest build is always the quire-x3-images artifact on the latest successful CI run on main.',
  },
  {
    q: 'Does it phone home?',
    a: 'No. There is no account, no telemetry and no DRM. The reading statistics are computed on the device from its own session log and never leave it. The only network traffic is what you ask for: the Drop page, a download, an update check, the weather or a feed.',
  },
  {
    q: 'What formats does it read?',
    a: 'EPUB and KEPUB, PDF, plain text, Markdown, FB2, HTML, CBZ comics, and Quire’s own .qbk. Documents are streamed from the card rather than loaded whole — there is no PSRAM on this chip.',
  },
  {
    q: 'What happens if an update fails?',
    a: 'An update is written to the other OTA slot, verified while it streams, read back and verified again, and only then selected. The firmware confirms itself valid after its first frame; three crashes in a row on an unconfirmed image roll back automatically. A 512 KB recovery app sits in the factory slot and can always reinstall from the card.',
  },
  {
    q: 'Do I need ESP-IDF or a C toolchain to build it?',
    a: 'No. Quire is Rust on a stable toolchain, no_std plus alloc, with no ESP-IDF and no C toolchain. cargo test --workspace runs the host tests and the simulator; just images builds the four release files.',
  },
  {
    q: 'Is there a licence I have to worry about?',
    a: 'Quire is dual-licensed MIT or Apache-2.0, at your option. The bundled fonts are under the SIL Open Font License, the dictionary is derived from WordNet 3.1 under its own licence, and the sleep packs are CC0.',
  },
]
