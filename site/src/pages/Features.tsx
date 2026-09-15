import { Link } from 'react-router-dom'
import { DeviceFrame } from '../components/DeviceFrame'
import { Reveal } from '../components/Reveal'
import { CARD_PATHS } from '../data/site'
import { useReveal } from '../lib/useReveal'
import { useSeo } from '../lib/useSeo'

export function Features() {
  useSeo({
    title: 'Features',
    description:
      'Everything Quire does on the Xteink X3: typesetting, eight document formats, the Drop page, an offline WordNet dictionary, apps and games, on-device analytics, sleep screens with a live clock, and verified OTA updates.',
    path: '/features',
  })
  useReveal([])

  return (
    <>
      <header className="page-head">
        <div className="wrap">
          <span className="eyebrow">Features</span>
          <h1>Everything it does, with the numbers.</h1>
          <p>
            Quire is finished firmware, not a demo. Each of these is implemented, tested in the
            simulator, and shown in the screen tour — nothing below is a mock or a plan.
          </p>
        </div>
      </header>

      {/* Reading -------------------------------------------------------- */}
      <section className="section" id="reading">
        <div className="wrap">
          <Reveal className="split">
            <div className="split__body">
              <span className="eyebrow">01 · Reading</span>
              <h2>The typesetter</h2>
              <p>
                Text is laid out by Quire’s own line breaker, not by a browser engine.
                Paragraphs are justified, hyphenated with the hypher patterns for seven
                languages, and paginated against the real panel, so a page never reflows
                differently between a repaint and the next boot.
              </p>
              <p>
                Literata ships as baked bitmap strikes — 22, 24, 26, 28, 31 and 34 px, regular,
                bold and italic — so the glyphs are pixel-exact on a 1-bit panel instead of being
                antialiased into grey that e-ink cannot show.
              </p>
              <dl className="facts">
                <div>
                  <dt>Sizes</dt>
                  <dd>Six, 22–34 px</dd>
                </div>
                <div>
                  <dt>Hyphenation</dt>
                  <dd>English, German, French, Spanish, Italian, Dutch, Portuguese</dd>
                </div>
                <div>
                  <dt>Font strikes in flash</dt>
                  <dd>1.40 MB, guarded by a test at 1.5 MB</dd>
                </div>
              </dl>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="layout-22" plain />
                <DeviceFrame file="layout-34" plain />
              </div>
            </div>
          </Reveal>

          <Reveal className="split split--flip">
            <div className="split__body">
              <h3>Moving through a book</h3>
              <p>
                Seven keys have to do everything, so the reader leans on two ideas: the
                <strong> Compass</strong>, which lists every action available on the page you are
                looking at, and <strong>Jump</strong>, one filterable list of everything on the
                device.
              </p>
              <ul className="ticks">
                <li>Skim: flick through pages and land back where you chose.</li>
                <li>Go to: a page number or a percentage.</li>
                <li>Contents, footnotes opened in place, highlights and notes.</li>
                <li>Spine: a progress strip that shows the shape of the whole book.</li>
                <li>The end-of-book page suggests what to read next from your own shelf.</li>
              </ul>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="24-compass" plain />
                <DeviceFrame file="21-skim" plain />
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* Library -------------------------------------------------------- */}
      <section className="section section--tinted" id="library">
        <div className="wrap">
          <Reveal>
            <div className="section-head">
              <span className="eyebrow">02 · Library and formats</span>
              <h2>Eight formats, streamed from the card</h2>
              <p>
                EPUB and KEPUB, PDF, plain text, Markdown, FB2, HTML, CBZ comics and Quire’s own
                <code> .qbk</code>. There is no PSRAM on this chip, so no reader ever loads a
                document whole: everything is read through the card filesystem on demand.
              </p>
            </div>
          </Reveal>

          <Reveal className="strip" delay={60}>
            <DeviceFrame file="11-library" plain />
            <DeviceFrame file="11-library-list" plain />
            <DeviceFrame file="12-bookinfo" plain />
            <DeviceFrame file="13-folders" plain />
            <DeviceFrame file="42-jump" plain />
            <DeviceFrame file="2A-endofbook" plain />
          </Reveal>

          <Reveal delay={100}>
            <h3 style={{ marginTop: 'var(--s-8)' }}>What Quire expects on the card</h3>
            <p className="measure">
              FAT32, any size. Books go in <code>/Books</code> — or <code>/books</code>, or the
              card root, in any folder structure you like.
            </p>
            <div className="table-scroll">
              <table>
                <caption className="visually-hidden">Folders Quire reads on the microSD card</caption>
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
          </Reveal>
        </div>
      </section>

      {/* Getting books -------------------------------------------------- */}
      <section className="section" id="books">
        <div className="wrap">
          <Reveal className="split">
            <div className="split__body">
              <span className="eyebrow">03 · Getting books</span>
              <h2>The Drop page</h2>
              <p>
                Join a 2.4 GHz network or turn on the reader’s own <code>Quire-XXXX</code>{' '}
                hotspot, then open <code>http://quire.local</code> from anything with a browser.
                One page does the lot:
              </p>
              <ul className="ticks">
                <li>Upload books straight onto the card.</li>
                <li>A live mirror of the panel, so you can see what the reader is showing.</li>
                <li>Type into any text field on the device from a phone keyboard.</li>
                <li>A JSON API for scripts.</li>
                <li>Fetch a link: give it a URL and the reader downloads it itself.</li>
              </ul>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="30-drop-full" plain />
                <DeviceFrame file="30-drop-connected" plain />
              </div>
            </div>
          </Reveal>

          <Reveal className="split split--flip">
            <div className="split__body">
              <h3>Catalogues and the rest of your setup</h3>
              <p>
                The <strong>Bookshop</strong> carries a curated catalogue of 88 public-domain
                books that the reader downloads itself. Beyond that, Quire speaks the protocols
                you already use: OPDS catalogues, Calibre’s wireless device, and KOReader progress
                sync so a phone and the reader agree on where you stopped.
              </p>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="35-bookshop" plain />
                <DeviceFrame file="32-opds" plain />
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* Dictionary ----------------------------------------------------- */}
      <section className="section section--tinted" id="dictionary">
        <div className="wrap">
          <Reveal className="split">
            <div className="split__body">
              <span className="eyebrow">04 · Dictionary</span>
              <h2>WordNet 3.1, compiled in</h2>
              <p>
                83,253 headwords in a 1.5 MB blob that lives in the assets partition rather than
                in the firmware image, because the chip can only map 4 MB of flash for code and
                constants. Lookups read a few hundred bytes at a time and peak under 16 KB of
                heap.
              </p>
              <p>
                Drop StarDict dictionaries into <code>/dict</code> on the card and Quire builds a
                small <code>.qix</code> index beside each one the first time it is used.
              </p>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="26-cursor" plain />
                <DeviceFrame file="27-dictionary" plain />
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* Apps and games ------------------------------------------------- */}
      <section className="section" id="apps">
        <div className="wrap">
          <Reveal>
            <div className="section-head">
              <span className="eyebrow">05 · Apps and games</span>
              <h2>The rest of the device</h2>
              <p>
                Clock with timer and alarms, weather from Open-Meteo, news from RSS and Atom
                feeds, Wikipedia, notes, flashcards, a calculator, an image viewer, and
                interactive fiction on a Z-machine interpreter. Games: chess, sudoku, minesweeper,
                2048 and Wordle.
              </p>
            </div>
          </Reveal>
          <Reveal className="strip" delay={60}>
            <DeviceFrame file="70-apps" plain />
            <DeviceFrame file="72-news-articles" plain />
            <DeviceFrame file="77-wikipedia" plain />
            <DeviceFrame file="71-flashcards-back" plain />
            <DeviceFrame file="76-fiction" plain />
            <DeviceFrame file="79-notes" plain />
          </Reveal>
          <Reveal className="strip" delay={100} >
            <DeviceFrame file="80-games" plain />
            <DeviceFrame file="80-chess" plain />
            <DeviceFrame file="80-sudoku" plain />
            <DeviceFrame file="80-minesweeper" plain />
            <DeviceFrame file="80-2048" plain />
            <DeviceFrame file="80-wordle" plain />
          </Reveal>
        </div>
      </section>

      {/* Analytics ------------------------------------------------------ */}
      <section className="section section--tinted" id="analytics">
        <div className="wrap">
          <Reveal className="split">
            <div className="split__body">
              <span className="eyebrow">06 · Analytics</span>
              <h2>Computed here, kept here</h2>
              <p>
                Quire keeps a session log on the card and reads it back for everything you see:
                reading rhythm by hour and weekday, a calendar, goals and streaks, pages per
                book, and a year in review. No account, no telemetry, nothing uploaded.
              </p>
              <p>
                The only network traffic the reader makes is traffic you asked for: the Drop
                page, a download, an update check, the weather, a feed.
              </p>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="60a-overview" plain />
                <DeviceFrame file="61-yearinreview" plain />
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* Sleep ---------------------------------------------------------- */}
      <section className="section" id="sleep">
        <div className="wrap">
          <Reveal className="split split--flip">
            <div className="split__body">
              <span className="eyebrow">07 · Sleep and power</span>
              <h2>The screen you leave behind</h2>
              <p>
                Cover, poster, a quote from where you stopped, quick resume, blank, or an image
                pack. With a pack the firmware keeps a live clock: during light sleep it wakes in
                30-second slices, re-reads the DS3231, and repaints only the clock rectangle,
                once a minute.
              </p>
              <p>
                Ten packs, fifty images, all CC0 — downloadable from the reader or copied into{' '}
                <code>/sleep/packs/</code> on the card. Loose <code>.pbm</code> images at 528 ×
                792 work too, with 1 as ink.
              </p>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="40-sleep-pack" plain />
                <DeviceFrame file="40-sleep-poster" plain />
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* Updates -------------------------------------------------------- */}
      <section className="section section--tinted" id="updates">
        <div className="wrap">
          <Reveal className="split">
            <div className="split__body">
              <span className="eyebrow">08 · Updates and recovery</span>
              <h2>Two slots and a way back</h2>
              <p>
                A new image goes to whichever OTA slot is not running. It is verified while it
                streams — ESP-IDF header, segment walk, checksum, appended SHA-256 — then read
                back and verified again before <code>otadata</code> selects it. The running
                firmware marks itself valid after its first frame; three crashes in a row on an
                unconfirmed image roll back to the previous slot.
              </p>
              <p>
                The recovery app sits in a 512 KB factory partition that OTA never touches. Hold{' '}
                <kbd>Back</kbd> while powering on, or choose <em>Settings → About → Restart into
                recovery</em>. It reports why it booted, what is in both slots and what is on the
                card, and offers Retry, Card install and Rollback.
              </p>
              <p>
                <Link className="link-arrow" to="/guide#recovery">
                  The recovery walkthrough
                </Link>
              </p>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="51-ota-working" plain />
                <DeviceFrame file="99-recovery" plain />
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* Hardware ------------------------------------------------------- */}
      <section className="section" id="hardware">
        <div className="wrap">
          <Reveal>
            <div className="section-head">
              <span className="eyebrow">09 · The hardware it drives</span>
              <h2>What is actually inside an X3</h2>
              <p>
                Quire talks to all of it directly: no ESP-IDF, no vendor SDK, no C in the build.
              </p>
            </div>
          </Reveal>
          <Reveal className="grid" delay={60}>
            <div className="card">
              <span className="card__num">SoC</span>
              <h3>ESP32-C3</h3>
              <p>
                Single-core RISC-V at 160 MHz, 320 KB SRAM (313 KB usable DRAM), no PSRAM, 16 MB
                flash.
              </p>
            </div>
            <div className="card">
              <span className="card__num">Panel</span>
              <h3>528 × 792, 1-bit</h3>
              <p>
                Two controller variants are probed at boot: the original UC8253 at 10 MHz SPI and
                the UC8279 at 20 MHz, on a bus shared with the card.
              </p>
            </div>
            <div className="card">
              <span className="card__num">Input</span>
              <h3>Seven keys</h3>
              <p>
                Back, Confirm, Left, Right, Up, Down and Power — read as two calibrated ADC
                ladders. Left-handed rotation flips the whole UI.
              </p>
            </div>
            <div className="card">
              <span className="card__num">Storage</span>
              <h3>microSD, FAT32</h3>
              <p>
                Any size. Books, sleep packs, dictionaries, notes, flashcards, stories and
                Quire’s own index all live on the card.
              </p>
            </div>
            <div className="card">
              <span className="card__num">Radio</span>
              <h3>Wi-Fi 2.4 GHz</h3>
              <p>
                Station or access point, started for a session and stopped when the transfer is
                over, because the radio needs most of the free heap.
              </p>
            </div>
            <div className="card">
              <span className="card__num">I²C</span>
              <h3>Clock, gauge, IMU</h3>
              <p>
                A DS3231 real-time clock, a BQ27220 battery gauge and a QMI8658 IMU for tilt and
                shake, at 400 kHz.
              </p>
            </div>
          </Reveal>
        </div>
      </section>

      <section className="section section--tinted">
        <div className="wrap cta">
          <Reveal>
            <h2>See it, then install it.</h2>
            <p>
              Every screen above is in the tour, rendered by the simulator from the same code the
              device runs.
            </p>
            <div className="cta__row">
              <Link className="btn" to="/screens">
                Browse the screens
              </Link>
              <Link className="btn btn--ghost" to="/downloads">
                Downloads
              </Link>
            </div>
          </Reveal>
        </div>
      </section>
    </>
  )
}
