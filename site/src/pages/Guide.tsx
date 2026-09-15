import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { Callout } from '../components/Callout'
import { CodeBlock } from '../components/CodeBlock'
import { DeviceFrame } from '../components/DeviceFrame'
import {
  ACTIONS_URL,
  CARD_PATHS,
  ESPFLASH_RELEASE_URL,
  ESPFLASH_VERSION,
  RELEASES_URL,
  REPO,
} from '../data/site'
import { useScrollSpy } from '../lib/useScrollSpy'
import { useSeo } from '../lib/useSeo'

const SECTIONS = [
  { id: 'start', label: 'Before you start' },
  { id: 'browser', label: 'Install from your browser' },
  { id: 'backup', label: 'Back up the stock firmware' },
  { id: 'terminal', label: 'The terminal route' },
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
      'Install Quire on an Xteink X3: do it from your browser or from a terminal, back up the stock firmware first, put books on the card, use the Drop page, update over Wi-Fi, and recover or roll back.',
    path: '/guide',
  })

  const active = useScrollSpy(IDS, 140)

  /* The terminal route is folded away by default. A link straight to one of the
     headings inside it — /guide#flash, from the Downloads page — has to open it. */
  const [terminalOpen, setTerminalOpen] = useState(false)
  useEffect(() => {
    const hash = window.location.hash
    if (hash === '#terminal' || hash === '#flash') setTerminalOpen(true)
  }, [])

  return (
    <>
      <header className="page-head">
        <div className="wrap">
          <span className="eyebrow">Guide</span>
          <h1>From a stock X3 to a reader running Quire.</h1>
          <p>
            The whole installation happens in your browser — there is a page for it on this
            site. Whichever route you take, copy out the firmware your reader came with first:
            it is the only way back, and it takes a few minutes.
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
          <section id="start">
            <span className="step">Step 01</span>
            <h2>Before you start</h2>
            <p>You need four things, and none of them is a programming tool.</p>
            <ul>
              <li>
                An <strong>Xteink X3</strong> and the pogo-pin cable that came with it — the one
                with the spring-loaded pins that press against the back of the reader.
              </li>
              <li>A computer with a USB port: Windows, macOS, Linux or ChromeOS.</li>
              <li>
                <strong>Chrome, Edge, or another Chromium-based browser</strong> on that
                computer. Firefox and Safari cannot talk to the cable. Neither can a phone or a
                tablet.
              </li>
              <li>A microSD card formatted FAT32, for your books. That part can wait.</li>
            </ul>
            <p>
              Set aside half an hour, most of which is waiting: copying the reader’s 16 MB of
              memory takes a few minutes, and writing Quire takes a couple more.
            </p>

            <Callout title="Every time you attach the cable">
              <p>
                <strong>Switch the X3 on first, then attach the pogo cable.</strong> A reader
                that is off or asleep does not announce itself to the computer, and nothing you
                try afterwards will find it.
              </p>
            </Callout>

            <h3 id="locked">Some readers cannot be flashed at all</h3>
            <p>
              The chip inside the X3 has a one-time switch that turns off the way firmware is
              loaded over the cable. It is called an eFuse: burnt once, never unburnt. Some X3
              units left the factory with that switch already burnt, and there is no way to tell
              from the outside.
            </p>
            <p>
              What you would see is this: the reader is switched on, the cable is attached, and
              no new device appears on the computer at all — no port, in any tool. If that
              happens on a second cable and a second USB socket, the fuse is the likely answer.
              Nothing can undo it, so the reader keeps the firmware it came with. Installing
              Quire on one of those is not something this project can do.
            </p>
          </section>

          {/* 2 ---------------------------------------------------------- */}
          <section id="browser">
            <span className="step">Step 02 · start here</span>
            <h2>Install from your browser</h2>
            <p>
              There is a page on this site that does the whole installation. It speaks to the
              reader through the cable, exactly as the command-line tools do, and there is
              nothing to install on your computer first.
            </p>
            <p>It asks for one thing at a time:</p>
            <ol>
              <li>
                <strong>Connect.</strong> You press a button; the browser shows a short list of
                ports; you choose the one that appeared when you attached the cable. The page
                then prints what it found — the chip, its address and how much memory it has.
              </li>
              <li>
                <strong>Back up.</strong> It copies all 16 MB out of the reader into a file and
                hands you that file, with a checksum beside it. This step only reads. It cannot
                change anything on the reader.
              </li>
              <li>
                <strong>Choose.</strong> You give it the Quire image. It checks the file is the
                right size and really is a flash image before it goes anywhere near the reader.
              </li>
              <li>
                <strong>Write.</strong> You tick a box to say your backup is safe, press once
                more, and it writes Quire and restarts the reader.
              </li>
            </ol>

            <p className="actions">
              <Link className="btn" to="/install">
                Open the installer
              </Link>
              <Link className="btn btn--ghost" to="/install#restore">
                Or put the stock firmware back
              </Link>
            </p>

            <Callout title="Nothing is written by accident">
              <p>
                Until you press a button that says it is about to write, the page only reads. If
                something goes wrong part-way it lets go of the cable and lets you start again
                without reloading.
              </p>
            </Callout>
          </section>

          {/* 3 ---------------------------------------------------------- */}
          <section id="backup">
            <span className="step">Step 03 · do not skip</span>
            <h2>Back up the firmware your reader came with</h2>

            <Callout title="This is the only way back" tone="warn">
              <p>
                The factory firmware is <strong>not published anywhere</strong>. Xteink does not
                hand out an image, and this project cannot pass one on to you. If you write Quire
                over it without first copying it out to a file, the reader you bought is gone.
              </p>
              <p>Whichever route you take, the copy comes first.</p>
            </Callout>

            <h3>What a finished backup looks like</h3>
            <ul>
              <li>
                One file, <strong>exactly 16,777,216 bytes</strong> — 16 MB to the byte. Anything
                shorter is a read that stopped early, not a backup. Do it again.
              </li>
              <li>
                A checksum saved next to it, so that in a year you can still tell the file is the
                file.
              </li>
              <li>
                A copy somewhere that is <em>not</em> the computer you are about to experiment
                on: a second disk, a USB stick, a cloud folder.
              </li>
            </ul>
            <p>
              The <Link to="/install">browser installer</Link> does all three for you: it checks
              the length, works out the checksum, and saves both files. The terminal route below
              does the same in two commands.
            </p>

            <h3>Putting it back</h3>
            <p>
              That one file is the reader as it shipped. Writing it back to address 0 — from the
              installer’s <Link to="/install#restore">Restore</Link> section, or from a terminal
              — undoes everything, at any time, as many times as you like.
            </p>
          </section>

          {/* 4 ---------------------------------------------------------- */}
          <section id="terminal">
            <span className="step">Step 04 · the other way</span>
            <h2>If you would rather use a terminal</h2>
            <p>
              Everything the installer page does can be done from a command line with{' '}
              <strong>espflash</strong>, or with esptool if you already have it. This is the
              route to take if you are scripting the job, and the fallback if the browser route
              misbehaves. The addresses and the file sizes are the same either way.
            </p>

            <details
              className="alt"
              open={terminalOpen}
              onToggle={(e) => setTerminalOpen(e.currentTarget.open)}
            >
              <summary>The terminal route, in full</summary>
              <div className="alt__body">
                <h3>Getting espflash</h3>
                <p>
                  Three ways, easiest first. Any of them gives you the same program; this project
                  is tested against version {ESPFLASH_VERSION}.
                </p>
                <ol>
                  <li>
                    <strong>Download a prebuilt binary.</strong> The espflash project attaches one
                    to every release: a <code>.zip</code> per platform, named for it — for
                    instance <code>espflash-x86_64-unknown-linux-gnu.zip</code> or{' '}
                    <code>espflash-aarch64-apple-darwin.zip</code>. Unzip it and put the{' '}
                    <code>espflash</code> program somewhere on your <code>PATH</code>. No Rust
                    needed.{' '}
                    <a href={ESPFLASH_RELEASE_URL} target="_blank" rel="noreferrer">
                      espflash {ESPFLASH_VERSION} downloads
                    </a>
                    .
                  </li>
                  <li>
                    <strong>Use esptool instead.</strong> If you already have Python and esptool,
                    every command below has an esptool equivalent beside it.
                  </li>
                  <li>
                    <strong>Build it with cargo.</strong> If you have a Rust toolchain — or want
                    one from{' '}
                    <a href="https://rustup.rs" target="_blank" rel="noreferrer">
                      rustup.rs
                    </a>{' '}
                    — install it pinned to the tested version.
                  </li>
                </ol>
                <CodeBlock lang="shell" code={`cargo install espflash@${ESPFLASH_VERSION} --locked`} />
                <p>
                  On Linux you may also need to be in the <code>dialout</code> group (or its
                  equivalent) before you can open the serial port.
                </p>

                <h3>Check the reader answers</h3>
                <p>
                  Power the X3 on, attach the cable, and ask the chip who it is. A working unit
                  prints its chip, MAC and flash size; if nothing appears on any cable or port,
                  see <a href="#locked">the note about locked units</a>.
                </p>
                <CodeBlock lang="shell" code={`espflash board-info`} />

                <h3>Read the whole flash</h3>
                <p>16 MB over the pogo cable takes a few minutes. Do not unplug it while it runs.</p>
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
                  The file must be <strong>exactly 16,777,216 bytes</strong>. Anything shorter is
                  a truncated read, not a backup — do it again.
                </p>
                <CodeBlock
                  lang="shell"
                  code={`# Linux
stat -c %s xteink-stock-16mb.bin      # must print 16777216

# macOS
stat -f %z xteink-stock-16mb.bin      # must print 16777216

# keep a checksum next to it
sha256sum xteink-stock-16mb.bin > xteink-stock-16mb.bin.sha256
# on macOS: shasum -a 256 xteink-stock-16mb.bin > xteink-stock-16mb.bin.sha256`}
                />

                <h3 id="flash">Write Quire</h3>
                <p>
                  Get the release files from the <Link to="/downloads">Downloads page</Link> —
                  either a published release or the <code>quire-x3-images</code> artifact from
                  the latest successful CI run — and unpack them into one folder. A first install
                  writes the whole 16 MB image: bootloader, partition table, recovery app,
                  firmware, dictionary and <code>otadata</code>.
                </p>
                <CodeBlock lang="shell" code={`espflash write-bin 0x0 quire-x3-factory.bin`} />

                <h4>Writing a single partition</h4>
                <p>
                  Once the factory image is on, the individual files can be written on their own
                  — useful when you are only replacing the firmware or the dictionary.
                </p>
                <CodeBlock
                  lang="shell"
                  code={`espflash write-bin 0xa0000  quire-x3.bin        # the firmware, into ota_0
espflash write-bin 0x20000  quire-recovery.bin  # the recovery app
espflash write-bin 0xca0000 quire-assets.bin    # the dictionary`}
                />

                <h3>Restore the stock firmware</h3>
                <p>
                  With the backup file you can put the reader back to how it shipped at any time,
                  from the same cable:
                </p>
                <CodeBlock
                  lang="shell"
                  code={`espflash write-bin 0x0 xteink-stock-16mb.bin

# or, with esptool
esptool.py --chip esp32c3 write_flash 0x0 xteink-stock-16mb.bin`}
                />

                <Callout title="If the write fails halfway">
                  <p>
                    Nothing is lost that a second attempt cannot fix: power the device on,
                    reattach the cable and run the same command again. Only a flash that has
                    never had a valid bootloader written needs the factory image rather than a
                    single partition.
                  </p>
                </Callout>
              </div>
            </details>
          </section>

          {/* 5 ---------------------------------------------------------- */}
          <section id="firstrun">
            <span className="step">Step 05</span>
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

          {/* 6 ---------------------------------------------------------- */}
          <section id="books">
            <span className="step">Step 06</span>
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

          {/* 7 ---------------------------------------------------------- */}
          <section id="drop">
            <span className="step">Step 07</span>
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

          {/* 8 ---------------------------------------------------------- */}
          <section id="catalogues">
            <span className="step">Step 08</span>
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

          {/* 9 ---------------------------------------------------------- */}
          <section id="extras">
            <span className="step">Step 09</span>
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

          {/* 10 ---------------------------------------------------------- */}
          <section id="updating">
            <span className="step">Step 10</span>
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

          {/* 11 --------------------------------------------------------- */}
          <section id="recovery">
            <span className="step">Step 11</span>
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
              If none of that helps, the pogo cable and your backup always will. Write the
              factory image again from the <Link to="/install">browser installer</Link>, or go
              back to the firmware the reader came with using the file from{' '}
              <a href="#backup">step 03</a> — the installer’s{' '}
              <Link to="/install#restore">Restore</Link> section takes it, and so does{' '}
              <code>espflash write-bin 0x0 xteink-stock-16mb.bin</code>.
            </p>
            <div style={{ maxWidth: '18rem', margin: 'var(--s-5) 0' }}>
              <DeviceFrame file="99-recovery" plain />
            </div>
          </section>

          {/* 12 --------------------------------------------------------- */}
          <section id="source">
            <span className="step">Step 12</span>
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

          {/* 13 --------------------------------------------------------- */}
          <section id="trouble">
            <span className="step">Step 13</span>
            <h2>Troubleshooting</h2>

            <h3>The Connect button says my browser cannot do this</h3>
            <p>
              Talking to the cable needs the Web Serial API, which today only Chromium-based
              browsers have: Chrome, Edge, Opera, Brave. Firefox and Safari do not, and no
              browser on a phone or tablet does. Either open{' '}
              <Link to="/install">the installer</Link> in one of those on a computer, or use{' '}
              <a href="#terminal">the terminal route</a>.
            </p>

            <h3>The list of ports is empty, or the reader is not in it</h3>
            <p>
              Switch the X3 on first, <em>then</em> attach the pogo cable, and check the pins are
              seated squarely. Try the cable in a USB socket on the computer itself rather than
              a hub. Close anything else that might be holding the port — a serial monitor, an
              IDE, a second tab of the installer. On Linux, confirm your user can open serial
              ports (usually the <code>dialout</code> group); on macOS, check the USB-serial
              driver enumerated the port. If no port ever appears, on any cable, the unit may be{' '}
              <a href="#locked">one of the locked ones</a>.
            </p>

            <h3>The backup file is smaller than 16,777,216 bytes</h3>
            <p>
              The read was interrupted. Delete the file and do it again — a partial backup is
              worse than none, because it looks like one. The installer checks this for you and
              refuses to call a short read a backup.
            </p>

            <h3>The write stopped part-way through</h3>
            <p>
              The reader is half-written, which sounds worse than it is: nothing is lost that a
              second attempt cannot fix. Power the device on, attach the cable, connect again and
              write the same file again from the beginning.
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
