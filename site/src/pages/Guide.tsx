import { Link } from 'react-router-dom'
import { Callout } from '../components/Callout'
import { CodeBlock } from '../components/CodeBlock'
import { DeviceFrame } from '../components/DeviceFrame'
import { ACTIONS_URL, CARD_PATHS, RELEASES_URL, REPO } from '../data/site'
import { useScrollSpy } from '../lib/useScrollSpy'
import { useSeo } from '../lib/useSeo'

const SECTIONS = [
  { id: 'requirements', label: 'What you need' },
  { id: 'backup', label: 'Back up the stock firmware' },
  { id: 'flash', label: 'Flash Quire' },
  { id: 'firstrun', label: 'First run' },
  { id: 'books', label: 'Books on the card' },
  { id: 'drop', label: 'The Drop page' },
  { id: 'catalogues', label: 'Bookshop, OPDS, Calibre' },
  { id: 'extras', label: 'Sleep packs and dictionaries' },
  { id: 'updating', label: 'Updating' },
  { id: 'recovery', label: 'Recovery and rollback' },
  { id: 'source', label: 'Building from source' },
  { id: 'trouble', label: 'Troubleshooting' },
] as const

const IDS = SECTIONS.map((s) => s.id)

export function Guide() {
  useSeo({
    title: 'Guide',
    description:
      'Install Quire on an Xteink X3: back up the stock firmware, flash the factory image with espflash, put books on the card, use the Drop page, update over Wi-Fi, and recover or roll back.',
    path: '/guide',
  })

  const active = useScrollSpy(IDS, 140)

  return (
    <>
      <header className="page-head">
        <div className="wrap">
          <span className="eyebrow">Guide</span>
          <h1>From a stock X3 to a reader running Quire.</h1>
          <p>
            Read step two before you do anything else. Backing up the flash you already have is
            the only way back to the factory firmware, and it takes a few minutes.
          </p>
        </div>
      </header>

      <div className="wrap guide">
        <nav className="toc" aria-label="On this page">
          <h2>On this page</h2>
          <ol>
            {SECTIONS.map((s) => (
              <li key={s.id}>
                <a href={`#${s.id}`} aria-current={active === s.id ? 'true' : undefined}>
                  {s.label}
                </a>
              </li>
            ))}
          </ol>
        </nav>

        <div className="prose">
          {/* 1 ---------------------------------------------------------- */}
          <section id="requirements">
            <span className="step">Step 01</span>
            <h2>What you need</h2>
            <ul>
              <li>An <strong>Xteink X3</strong> and its pogo-pin cable.</li>
              <li>A computer with a USB port, on Linux, macOS or Windows.</li>
              <li>A microSD card formatted FAT32.</li>
              <li>
                About 1 GB of free disk space: the backup alone is 16 MB, but the source build
                needs more.
              </li>
              <li>
                <strong>espflash 4.6.0</strong>, or esptool if you already have it.
              </li>
            </ul>

            <h3>Installing espflash</h3>
            <p>
              espflash is a Rust program; install it with cargo, pinned to the version this
              project is tested against.
            </p>
            <CodeBlock lang="shell" code={`cargo install espflash@4.6.0 --locked`} />
            <p>
              If you have no Rust toolchain yet, install one from{' '}
              <a href="https://rustup.rs" target="_blank" rel="noreferrer">
                rustup.rs
              </a>{' '}
              first. On Linux you may also need to be in the <code>dialout</code> group (or
              equivalent) to open the serial port.
            </p>

            <Callout title="Before every cable connection">
              <p>
                <strong>Power the X3 on before you attach the pogo cable.</strong> The device has
                to be awake for the connection to enumerate.
              </p>
            </Callout>
          </section>

          {/* 2 ---------------------------------------------------------- */}
          <section id="backup">
            <span className="step">Step 02 · do not skip</span>
            <h2>Back up the stock firmware first</h2>

            <Callout title="This is the only way back" tone="warn">
              <p>
                The factory firmware is <strong>not published anywhere</strong>. Xteink does not
                distribute an image, and this project cannot redistribute one. If you flash Quire
                without reading your own 16 MB flash to a file first, there is no way to put the
                reader back the way it came.
              </p>
              <p>
                Take the backup, verify its size, keep a checksum, and store the file somewhere
                you will still have it in a year.
              </p>
            </Callout>

            <h3>Read the whole flash</h3>
            <p>
              16 MB over the pogo cable takes a few minutes. Do not unplug it while it runs.
            </p>
            <CodeBlock
              lang="shell"
              code={`espflash read-flash 0x0 0x1000000 xteink-stock-16mb.bin`}
            />

            <p>If you would rather use esptool:</p>
            <CodeBlock
              lang="shell"
              code={`esptool.py --chip esp32c3 -b 460800 read_flash 0x0 0x1000000 xteink-stock-16mb.bin`}
            />

            <h3>Verify the file</h3>
            <p>
              The file must be <strong>exactly 16,777,216 bytes</strong>. Anything shorter is a
              truncated read, not a backup — do it again.
            </p>
            <CodeBlock
              lang="shell"
              code={`# Linux
stat -c %s xteink-stock-16mb.bin      # must print 16777216

# macOS
stat -f %z xteink-stock-16mb.bin      # must print 16777216

# keep a checksum next to it
sha256sum xteink-stock-16mb.bin > xteink-stock-16mb.bin.sha256`}
            />

            <h3>Restoring it later</h3>
            <p>
              With that file you can put the reader back to how it shipped at any time, from the
              same cable:
            </p>
            <CodeBlock
              lang="shell"
              code={`espflash write-bin 0x0 xteink-stock-16mb.bin

# or, with esptool
esptool.py write_flash 0x0 xteink-stock-16mb.bin`}
            />
            <p>
              Copy the <code>.bin</code> and its <code>.sha256</code> off your machine — a second
              disk, a cloud folder, anywhere that is not the laptop you are about to experiment
              on.
            </p>
          </section>

          {/* 3 ---------------------------------------------------------- */}
          <section id="flash">
            <span className="step">Step 03</span>
            <h2>Flash Quire</h2>
            <p>
              Get the four release files from the{' '}
              <Link to="/downloads">Downloads page</Link> — either a published release or the{' '}
              <code>quire-x3-images</code> artifact from the latest successful CI run — and
              unpack them into one folder.
            </p>
            <p>
              A first install writes the whole 16 MB image: bootloader, partition table, recovery
              app, firmware, dictionary and <code>otadata</code>.
            </p>
            <CodeBlock lang="shell" code={`espflash write-bin 0x0 quire-x3-factory.bin`} />

            <h3>Writing a single partition</h3>
            <p>
              Once the factory image is on, the individual files can be written on their own —
              useful when you are only replacing the firmware or the dictionary.
            </p>
            <CodeBlock
              lang="shell"
              code={`espflash write-bin 0xa0000  quire-x3.bin        # the firmware, into ota_0
espflash write-bin 0x20000  quire-recovery.bin  # the recovery app
espflash write-bin 0xca0000 quire-assets.bin    # the dictionary`}
            />

            <Callout title="If the write fails halfway">
              <p>
                Nothing is lost that a second attempt cannot fix: power the device on, reattach
                the cable and run the same command again. Only a flash that has never had a valid
                bootloader written needs the factory image rather than a single partition.
              </p>
            </Callout>
          </section>

          {/* 4 ---------------------------------------------------------- */}
          <section id="firstrun">
            <span className="step">Step 04</span>
            <h2>First run</h2>
            <p>
              Power the reader on. Quire asks four things: the time, the hyphenation language,
              the reading defaults, and where books live on the card. All of them are in{' '}
              <em>Settings</em> afterwards, so none of the answers are permanent.
            </p>
            <div className="strip" style={{ margin: 'var(--s-5) 0' }}>
              <DeviceFrame file="02-firstrun-time" plain />
              <DeviceFrame file="02-firstrun-language" plain />
              <DeviceFrame file="02-firstrun-reader" plain />
              <DeviceFrame file="02-firstrun-books" plain />
              <DeviceFrame file="10-home" plain />
              <DeviceFrame file="11-library" plain />
            </div>
            <p>
              The seven keys are Back, Confirm, the four directions and Power. Two things are
              worth learning on day one: <strong>Compass</strong> lists every action available on
              the screen you are looking at, and <strong>Jump</strong> is one filterable list of
              everything on the device.
            </p>
          </section>

          {/* 5 ---------------------------------------------------------- */}
          <section id="books">
            <span className="step">Step 05</span>
            <h2>Books on the card</h2>
            <p>
              Format the card FAT32 — any size works — and copy books into <code>/Books</code>.
              Subfolders are fine; Quire indexes the tree and caches covers under{' '}
              <code>/.quire</code>.
            </p>
            <div className="table-scroll">
              <table>
                <thead>
                  <tr>
                    <th scope="col">Path</th>
                    <th scope="col">Contents</th>
                  </tr>
                </thead>
                <tbody>
                  {CARD_PATHS.map((row) => (
                    <tr key={row.path}>
                      <td>
                        <code className="num">{row.path}</code>
                      </td>
                      <td>{row.holds}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p>
              Formats: EPUB and KEPUB, PDF, TXT, Markdown, FB2, HTML, CBZ and{' '}
              <code>.qbk</code>. Nothing needs converting first.
            </p>
          </section>

          {/* 6 ---------------------------------------------------------- */}
          <section id="drop">
            <span className="step">Step 06</span>
            <h2>The Drop page, over Wi-Fi or the reader’s hotspot</h2>
            <p>There are two ways to reach the Drop page, and they work the same once you are in.</p>

            <h3>On your own network</h3>
            <ol>
              <li>
                <em>Settings → Wi-Fi</em>, scan, choose the 2.4 GHz network and enter the
                password on the on-screen keyboard.
              </li>
              <li>
                Open the Drop page on the reader. It shows the address to visit — normally{' '}
                <code>http://quire.local</code>, with the numeric address underneath in case mDNS
                is blocked on your network.
              </li>
            </ol>

            <h3>Without a network</h3>
            <ol>
              <li>
                On the Drop page choose <em>Hotspot</em>. The reader raises its own{' '}
                <code>Quire-XXXX</code> access point and prints the network name, the password
                and two QR codes: one to join, one to open the page.
              </li>
              <li>
                Join it from your phone and open the address shown — the reader serves the page
                itself at <code>192.168.4.1</code>.
              </li>
            </ol>

            <p>From the page you can:</p>
            <ul>
              <li>Upload books straight onto the card.</li>
              <li>Watch a live mirror of the panel.</li>
              <li>Type into any text field on the device using the phone keyboard.</li>
              <li>Paste a URL into <em>Fetch a link</em> and let the reader download it.</li>
              <li>Drive the same endpoints from a script through the JSON API.</li>
            </ul>

            <div className="strip" style={{ margin: 'var(--s-5) 0' }}>
              <DeviceFrame file="31-wifi-scan" plain />
              <DeviceFrame file="31-wifi-password" plain />
              <DeviceFrame file="30-drop-connected" plain />
              <DeviceFrame file="30-drop-hotspot" plain />
              <DeviceFrame file="30-drop-full" plain />
              <DeviceFrame file="39-downloads" plain />
            </div>

            <Callout title="The radio is a session, not a state">
              <p>
                Wi-Fi needs most of the free heap, so Quire starts the radio for a transfer and
                stops it when the transfer is done. If the Drop page disappears, open it again —
                nothing has broken.
              </p>
            </Callout>
          </section>

          {/* 7 ---------------------------------------------------------- */}
          <section id="catalogues">
            <span className="step">Step 07</span>
            <h2>Bookshop, OPDS, Calibre and KOReader</h2>
            <h3>Bookshop</h3>
            <p>
              A curated catalogue of 88 public-domain books, browsable by shelf and searchable on
              the device. Choose a book and the reader downloads it itself.
            </p>
            <h3>OPDS</h3>
            <p>
              Add the catalogue’s URL under <em>Settings → Catalogues</em> and browse it like any
              other shelf. Authentication and paginated feeds are handled.
            </p>
            <h3>Calibre</h3>
            <p>
              Quire presents itself as a Calibre wireless device. Start Calibre’s{' '}
              <em>Connect / Share → Start wireless device connection</em> on the same network and
              send books to the reader from the desktop.
            </p>
            <h3>KOReader progress sync</h3>
            <p>
              Point Quire at a KOReader sync server with the same account you use on a phone or
              another reader, and the two agree on where you stopped in a book.
            </p>
          </section>

          {/* 8 ---------------------------------------------------------- */}
          <section id="extras">
            <span className="step">Step 08</span>
            <h2>Sleep packs and dictionaries</h2>
            <h3>Sleep packs</h3>
            <p>
              Ten CC0 packs of fifty images ship with the project. Download them from the reader,
              or copy them from <code>sleep-packs/</code> in the repository into{' '}
              <code>/sleep/packs/&lt;id&gt;/</code> on the card — each pack is a{' '}
              <code>pack.json</code> plus <code>NN.pbm</code> or <code>NN.pbm.z</code> files.
            </p>
            <p>
              Your own images work too: 528 × 792 <code>.pbm</code>, 1 as ink, dropped in{' '}
              <code>/sleep</code>. Pick one under <em>Settings → Sleep and power</em>. With a pack
              selected, the firmware keeps a live clock, repainting only the clock rectangle once
              a minute in light sleep.
            </p>
            <h3>Dictionaries</h3>
            <p>
              WordNet 3.1 is already installed in the assets partition — 83,253 headwords, no
              setup. For another language, put a StarDict dictionary’s <code>.ifo</code>,{' '}
              <code>.idx</code> and <code>.dict</code> files in <code>/dict</code> on the card;
              Quire builds a small <code>.qix</code> index beside each one on first use.
            </p>
          </section>

          {/* 9 ---------------------------------------------------------- */}
          <section id="updating">
            <span className="step">Step 09</span>
            <h2>Updating</h2>
            <h3>Over Wi-Fi</h3>
            <p>
              <em>Settings → About → Check for update</em>. The reader reads this repository’s
              GitHub releases, compares versions, and verifies the SHA-256 the release publishes
              before it does anything with the image.
            </p>
            <h3>From the card</h3>
            <p>
              Copy <code>quire-x3.bin</code> to the card as <code>/quire/update.bin</code>, then
              choose <em>Settings → About → Install from card</em>.
            </p>
            <h3>What happens then</h3>
            <p>
              The image is written to whichever OTA slot is not running and verified while it
              streams: the ESP-IDF header, a walk of every segment, the checksum, and the
              appended SHA-256. It is then read back and verified again before{' '}
              <code>otadata</code> selects it as <em>New</em>. After the reboot, the firmware
              marks itself <em>Valid</em> once it has painted its first frame. Three crashes in a
              row on an image that has not confirmed itself roll the reader back to the previous
              slot with no help from you.
            </p>
            <div className="strip" style={{ margin: 'var(--s-5) 0' }}>
              <DeviceFrame file="50-about" plain />
              <DeviceFrame file="51-ota-available" plain />
              <DeviceFrame file="51-ota-working" plain />
            </div>
          </section>

          {/* 10 --------------------------------------------------------- */}
          <section id="recovery">
            <span className="step">Step 10</span>
            <h2>Recovery and rollback</h2>
            <p>
              A 512 KB factory partition holds <code>quire-recovery</code>, which OTA never
              overwrites. Two ways in:
            </p>
            <ul>
              <li>Hold <kbd>Back</kbd> while powering the reader on.</li>
              <li><em>Settings → About → Restart into recovery</em>.</li>
            </ul>
            <p>The recovery app tells you why it booted, what is in both OTA slots and what it can see on the card, and offers three actions:</p>
            <ul>
              <li><strong>Retry</strong> — boot the selected slot again.</li>
              <li><strong>Card install</strong> — write <code>/quire/update.bin</code> from the card into the inactive slot.</li>
              <li><strong>Rollback</strong> — go back to the other slot.</li>
            </ul>
            <p>
              If none of that helps, the pogo cable and your backup always will: rewrite the
              factory image with <code>espflash write-bin 0x0 quire-x3-factory.bin</code>, or go
              back to stock with the file from step 02.
            </p>
            <div style={{ maxWidth: '18rem', margin: 'var(--s-5) 0' }}>
              <DeviceFrame file="99-recovery" plain />
            </div>
          </section>

          {/* 11 --------------------------------------------------------- */}
          <section id="source">
            <span className="step">Step 11</span>
            <h2>Building from source</h2>
            <p>
              Stable Rust, no ESP-IDF and no C toolchain. The host workspace carries the engine
              crates, the desktop tools and the simulator; <code>firmware/device/</code> is a
              separate workspace for the ESP32-C3 binaries.
            </p>
            <CodeBlock
              lang="shell"
              code={`git clone ${REPO}.git
cd quire/firmware

# host tests, including the 142-screen snapshot tour
cargo test --workspace

# render the tour to PNG
cargo run -p quire-sim -- --snapshots ./shots`}
            />
            <p>Then the device side:</p>
            <CodeBlock
              lang="shell"
              code={`cd device

# build both binaries
cargo build --release

# flash ota_0 over the pogo cable and attach a monitor
cargo run --release`}
            />
            <p>
              To produce the four release files exactly as CI does, run <code>just images</code>{' '}
              from <code>firmware/</code>.
            </p>
            <CodeBlock lang="shell" code={`cd .. && just images`} />
          </section>

          {/* 12 --------------------------------------------------------- */}
          <section id="trouble">
            <span className="step">Step 12</span>
            <h2>Troubleshooting</h2>

            <h3>espflash cannot see the device</h3>
            <p>
              Power the X3 on <em>before</em> attaching the pogo cable, and check the pins are
              seated. On Linux, confirm your user can open the serial port; on macOS, check that
              the USB-serial driver enumerated the port.
            </p>

            <h3>The backup file is smaller than 16,777,216 bytes</h3>
            <p>
              The read was interrupted. Delete the file and run the command again — a partial
              backup is worse than none, because it looks like one.
            </p>

            <h3>quire.local does not resolve</h3>
            <p>
              Some networks block mDNS. The Drop page prints the numeric address underneath the
              name; use that. On the reader’s own hotspot it is always{' '}
              <code>192.168.4.1</code>.
            </p>

            <h3>The reader will not join a network</h3>
            <p>
              The radio is 2.4 GHz only. A 5 GHz-only SSID will not appear in the scan; on a
              dual-band router, make sure the 2.4 GHz band is enabled and broadcasting.
            </p>

            <h3>The card is not read</h3>
            <p>
              The card must be FAT32. exFAT cards are not read; reformat, or use a smaller card
              that formats as FAT32 by default.
            </p>

            <h3>A book will not open</h3>
            <p>
              DRM-protected files are not supported and never will be. Check the file opens in
              another reader, and look at <em>Settings → Developer</em> for what the parser
              reported.
            </p>

            <h3>After an update the reader keeps restarting</h3>
            <p>
              Leave it alone: three crashes in a row on an unconfirmed image roll back to the
              previous slot by themselves. If it does not, hold <kbd>Back</kbd> while powering on
              and choose <strong>Rollback</strong> in the recovery app.
            </p>

            <h3>Something else</h3>
            <p>
              Open an issue on{' '}
              <a href={`${REPO}/issues`} target="_blank" rel="noreferrer">
                the repository
              </a>
              . Builds and logs live with the{' '}
              <a href={ACTIONS_URL} target="_blank" rel="noreferrer">
                CI runs
              </a>{' '}
              and the{' '}
              <a href={RELEASES_URL} target="_blank" rel="noreferrer">
                releases
              </a>
              .
            </p>
          </section>
        </div>
      </div>
    </>
  )
}
