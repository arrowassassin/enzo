import { Link } from 'react-router-dom'
import { DeviceFrame } from '../components/DeviceFrame'
import { Reveal } from '../components/Reveal'
import { DownloadIcon } from '../components/Icons'
import { SPECS, VERSION } from '../data/site'
import { screenCount } from '../lib/catalogue'
import { useReveal } from '../lib/useReveal'
import { useSeo } from '../lib/useSeo'

const TEASER = [
  '11-library',
  '25-type',
  '30-drop-full',
  '60b-rhythm',
  '80-chess',
  '99-recovery',
]

export function Home() {
  useSeo({
    title: 'Quire',
    description:
      'Quire is open-source e-reader firmware for the Xteink X3, written in Rust from scratch: black ink on white paper, seven keys, the book is the home screen.',
    path: '/',
  })
  useReveal([])

  return (
    <>
      {/* Hero ---------------------------------------------------------- */}
      <section className="hero">
        <div className="wrap hero__inner">
          <div>
            <span className="eyebrow">Open source · Rust · no_std</span>
            <h1>Quire</h1>
            <p className="hero__sub">E-reader firmware for the Xteink&nbsp;X3</p>
            <p className="hero__lead">
              Black ink on white paper, seven keys, and the book is the home screen. Written
              from scratch in Rust — the typesetter, every screen, the dictionary, the network
              stack, right down to the certificate verifier.
            </p>
            <div className="hero__cta">
              <Link className="btn" to="/install">
                Install from your browser
              </Link>
              <Link className="btn btn--ghost" to="/downloads">
                <DownloadIcon />
                Downloads
              </Link>
            </div>
            <p className="hero__note">
              Version {VERSION} · MIT or Apache-2.0 · back up your stock firmware before you
              flash anything.
            </p>
          </div>

          <div className="hero__media">
            <DeviceFrame file="20-reading" size="lg" priority />
          </div>
        </div>
      </section>

      {/* Spec strip ---------------------------------------------------- */}
      <section className="specs" aria-label="Hardware at a glance">
        <div className="wrap">
          <ul className="specs__list">
            {SPECS.map((spec) => (
              <li key={spec.label}>
                <span className="specs__k">{spec.value}</span>
                <span className="specs__v">{spec.label}</span>
              </li>
            ))}
          </ul>
        </div>
      </section>

      {/* Reading ------------------------------------------------------- */}
      <section className="section">
        <div className="wrap">
          <Reveal className="split">
            <div className="split__body">
              <span className="eyebrow">Reading</span>
              <h2>Typesetting, not text rendering.</h2>
              <p>
                Literata at six sizes from 22 to 34 px, with real bold and italic strikes rather
                than synthesised ones. Lines are justified and hyphenated in seven languages,
                chapters open on a drop cap, footnotes open in place, and images and small-caps
                headers land where the book asked for them.
              </p>
              <ul className="ticks">
                <li>English, German, French, Spanish, Italian, Dutch and Portuguese hyphenation.</li>
                <li>Spine: a progress strip that shows the shape of the whole book.</li>
                <li>Skim, Go to, Contents, highlights and notes — all on seven keys.</li>
              </ul>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="layout-chapter" plain />
                <DeviceFrame file="25-type" plain />
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* Formats ------------------------------------------------------- */}
      <section className="section section--ruled">
        <div className="wrap">
          <Reveal className="split split--flip">
            <div className="split__body">
              <span className="eyebrow">Formats</span>
              <h2>Every format you actually have. Including PDF.</h2>
              <p>
                EPUB and KEPUB, PDF, plain text, Markdown, FB2, HTML, CBZ comics, and Quire’s own
                <code> .qbk</code>. Documents are streamed from the card and paginated on the fly:
                the ESP32-C3 has no PSRAM, so nothing is ever loaded whole.
              </p>
              <p>
                Covers, series and authors are indexed once and cached under <code>/.quire</code>,
                so the shelf opens instantly on the next boot.
              </p>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="11-library" plain />
                <DeviceFrame file="11-library-list" plain />
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* Drop ---------------------------------------------------------- */}
      <section className="section section--tinted">
        <div className="wrap">
          <Reveal className="split">
            <div className="split__body">
              <span className="eyebrow">Getting books</span>
              <h2>A card slot, or a web page on your phone.</h2>
              <p>
                Join your Wi-Fi — or let the reader raise its own <code>Quire-XXXX</code> hotspot —
                and open <code>http://quire.local</code>. The Drop page uploads books, mirrors the
                screen live, lets you type into any field on the device from a phone keyboard,
                exposes a JSON API, and will fetch a link so the reader downloads it itself.
              </p>
              <ul className="ticks">
                <li>Bookshop: a curated catalogue of 88 public-domain books.</li>
                <li>OPDS catalogues, Calibre wireless device, KOReader progress sync.</li>
                <li>The radio runs as a session and shuts down when the transfer is done.</li>
              </ul>
            </div>
            <div className="split__media">
              <DeviceFrame file="30-drop-full" />
            </div>
          </Reveal>
        </div>
      </section>

      {/* Dictionary ---------------------------------------------------- */}
      <section className="section">
        <div className="wrap">
          <Reveal className="split split--flip">
            <div className="split__body">
              <span className="eyebrow">Dictionary</span>
              <h2>83,253 headwords, on the device, offline.</h2>
              <p>
                WordNet 3.1 is compiled into a 1.5 MB blob in its own flash partition and read a
                few hundred bytes at a time: a lookup peaks under 16 KB of heap. Move the cursor
                onto a word and press Confirm. StarDict dictionaries dropped on the card work the
                same way.
              </p>
              <dl className="facts">
                <div>
                  <dt>Headwords</dt>
                  <dd>83,253</dd>
                </div>
                <div>
                  <dt>Blob</dt>
                  <dd>1.5 MB in the assets partition</dd>
                </div>
                <div>
                  <dt>Peak heap per lookup</dt>
                  <dd>under 16 KB</dd>
                </div>
              </dl>
            </div>
            <div className="split__media">
              <DeviceFrame file="27-dictionary" />
            </div>
          </Reveal>
        </div>
      </section>

      {/* Apps and games ------------------------------------------------ */}
      <section className="section section--ruled">
        <div className="wrap">
          <Reveal>
            <div className="section-head">
              <span className="eyebrow">Apps and games</span>
              <h2>What else fits on a 528 × 792 panel.</h2>
              <p>
                A clock with timers and alarms, weather, news from your feeds, Wikipedia, notes,
                flashcards, a calculator, an image viewer and Z-machine interactive fiction —
                plus chess, sudoku, minesweeper, 2048 and Wordle.
              </p>
            </div>
          </Reveal>
          <Reveal className="strip" delay={80}>
            <DeviceFrame file="80-chess" plain />
            <DeviceFrame file="73-clock" plain />
            <DeviceFrame file="78-calculator" plain />
            <DeviceFrame file="74-weather" plain />
            <DeviceFrame file="76-fiction-play" plain />
            <DeviceFrame file="80-wordle" plain />
          </Reveal>
        </div>
      </section>

      {/* Analytics ----------------------------------------------------- */}
      <section className="section section--tinted">
        <div className="wrap">
          <Reveal className="split">
            <div className="split__body">
              <span className="eyebrow">Analytics</span>
              <h2>Statistics that never leave the reader.</h2>
              <p>
                Reading rhythm by hour and weekday, a calendar, goals and streaks, pages per book
                and a year in review — all computed on the device from its own session log. There
                is no account, no telemetry and no server to opt out of.
              </p>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="60b-rhythm" plain />
                <DeviceFrame file="60e-goals" plain />
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* Sleep --------------------------------------------------------- */}
      <section className="section">
        <div className="wrap">
          <Reveal className="split split--flip">
            <div className="split__body">
              <span className="eyebrow">Sleep screens</span>
              <h2>A panel that holds an image costs nothing to look at.</h2>
              <p>
                Choose the cover, a poster, a quote from where you stopped, a quick resume, a
                blank page, or an image pack with a live clock: in light sleep the firmware
                repaints only the clock rectangle, once a minute. Ten CC0 packs, fifty images,
                downloadable from the reader or copied to the card.
              </p>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="40-sleep-pack" plain />
                <DeviceFrame file="40-sleep-quote" plain />
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* Updates ------------------------------------------------------- */}
      <section className="section section--ruled">
        <div className="wrap">
          <Reveal className="split">
            <div className="split__body">
              <span className="eyebrow">Updates and recovery</span>
              <h2>A failed update should not cost you a reader.</h2>
              <p>
                Updates arrive over Wi-Fi or from <code>/quire/update.bin</code> on the card. The
                image is written to the other OTA slot, verified while it streams — ESP-IDF
                header, segment walk, checksum, appended SHA-256 — read back, verified again, and
                only then selected as <em>New</em>. The firmware confirms itself valid after its
                first frame; three crashes in a row on an unconfirmed image roll back by
                themselves.
              </p>
              <p>
                Behind that sits a 512 KB recovery partition. Hold <kbd>Back</kbd> while powering
                on and the recovery app tells you why it booted, what is in both slots, and offers
                Retry, Card install and Rollback.
              </p>
            </div>
            <div className="split__media">
              <div className="pair">
                <DeviceFrame file="51-ota-available" plain />
                <DeviceFrame file="99-recovery" plain />
              </div>
            </div>
          </Reveal>
        </div>
      </section>

      {/* Screens teaser ------------------------------------------------ */}
      <section className="section section--tight section--tinted">
        <div className="wrap">
          <Reveal>
            <div className="teaser">
              <div>
                <span className="eyebrow">The tour</span>
                <h2>{screenCount} screenshots. All of them real.</h2>
              </div>
              <Link className="link-arrow" to="/screens">
                Browse every screen
              </Link>
            </div>
          </Reveal>
          <Reveal className="strip" delay={60}>
            {TEASER.map((file) => (
              <DeviceFrame key={file} file={file} plain />
            ))}
          </Reveal>
        </div>
      </section>

      {/* Built properly ------------------------------------------------ */}
      <section className="section">
        <div className="wrap">
          <Reveal>
            <div className="section-head">
              <span className="eyebrow">Built properly</span>
              <h2>Nothing here is a mock.</h2>
              <p>
                Quire is a single Rust workspace on a stable toolchain: no ESP-IDF, no C
                toolchain, and no vendored blob doing the hard parts.
              </p>
            </div>
          </Reveal>
          <Reveal className="engineering" delay={60}>
            <div>
              <span className="card__num">01</span>
              <h3>Rust, no_std + alloc</h3>
              <p>
                esp-hal 1.1, esp-rtos 0.3, esp-radio 0.18, embassy-net 0.9 and picoserve 0.20 —
                the newest set of those crates that agree with each other.
              </p>
            </div>
            <div>
              <span className="card__num">02</span>
              <h3>Its own TLS verifier</h3>
              <p>
                TLS 1.3 with a certificate verifier written for this project: chain to a
                compiled-in root store, SAN matching, path-length and name constraints, RSA
                PKCS#1 v1.5 and PSS, ECDSA P-256 and P-384.
              </p>
            </div>
            <div>
              <span className="card__num">03</span>
              <h3>A 4 MB ceiling, not a suggestion</h3>
              <p>
                The ESP32-C3 maps at most 4 MB of flash for code and constants together. The
                image uses 4.11 MB of the 4.19 MB available — which is why the dictionary lives
                in its own partition and is read a few hundred bytes at a time.
              </p>
            </div>
            <div>
              <span className="card__num">04</span>
              <h3>Tested like firmware</h3>
              <p>
                A 142-screen snapshot tour in a headless simulator, plus grammar, fuzz,
                persistence, overflow and efficiency suites, 82 network protocol tests and 21
                board tests.
              </p>
            </div>
            <div>
              <span className="card__num">05</span>
              <h3>CI that builds the thing</h3>
              <p>
                Every push runs fmt, clippy with <code>-D warnings</code> and the whole suite in
                debug mode — integer overflow checks on — then builds both firmware binaries and
                publishes the images.
              </p>
            </div>
            <div>
              <span className="card__num">06</span>
              <h3>Memory you can account for</h3>
              <p>
                144 KB main heap, a 52 KB panel plane, a 40 KB main stack. The reading page holds
                about 115 KB of heap; a Wi-Fi session takes most of the rest, which is why the
                radio starts and stops around each transfer.
              </p>
            </div>
          </Reveal>
        </div>
      </section>

      {/* Closing ------------------------------------------------------- */}
      <section className="section section--tinted">
        <div className="wrap cta">
          <Reveal>
            <h2>Put it on a reader.</h2>
            <p>
              A backup of your stock firmware taken first, then one image written. It happens in
              a browser tab; the guide walks the whole thing, including how to get back.
            </p>
            <div className="cta__row">
              <Link className="btn" to="/install">
                Install from your browser
              </Link>
              <Link className="btn btn--ghost" to="/guide">
                Read the guide
              </Link>
            </div>
          </Reveal>
        </div>
      </section>
    </>
  )
}
