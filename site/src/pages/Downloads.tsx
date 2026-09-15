import { Link } from 'react-router-dom'
import { Callout } from '../components/Callout'
import { CodeBlock } from '../components/CodeBlock'
import { DownloadIcon } from '../components/Icons'
import {
  ACTIONS_URL,
  DEVICES,
  PARTITIONS,
  RELEASES_URL,
  RELEASE_FILES,
  VERSION,
} from '../data/site'
import { useSeo } from '../lib/useSeo'

const fmt = new Intl.NumberFormat('en-GB')

function mib(bytes: number): string {
  return `${(bytes / 1024 / 1024).toFixed(bytes >= 1024 * 1024 * 10 ? 1 : 2)} MB`
}

export function Downloads() {
  useSeo({
    title: 'Downloads',
    description:
      'The four Quire release files for the Xteink X3, what each one is, where it flashes and its exact size — plus where to get the newest build and how to verify it.',
    path: '/downloads',
  })

  return (
    <>
      <header className="page-head">
        <div className="wrap">
          <span className="eyebrow">Downloads</span>
          <h1>Four files, and one thing to do first.</h1>
          <p>
            Everything below writes over the firmware your reader shipped with. Read the backup
            step before you download anything.
          </p>
        </div>
      </header>

      <section className="section section--tight">
        <div className="wrap">
          <Callout title="Back up the stock firmware first" tone="warn">
            <p>
              The factory firmware is not published anywhere and cannot be redistributed by this
              project. Reading your own 16 MB flash to a file{' '}
              <strong>before you flash anything</strong> is the only way back to the reader you
              bought.
            </p>
            <CodeBlock
              lang="shell"
              code={`espflash read-flash 0x0 0x1000000 xteink-stock-16mb.bin
# the file must be exactly 16777216 bytes
sha256sum xteink-stock-16mb.bin > xteink-stock-16mb.bin.sha256`}
            />
            <p>
              <Link className="link-arrow" to="/guide#backup">
                The full backup and restore steps
              </Link>
            </p>
          </Callout>
        </div>
      </section>

      {/* Current build ------------------------------------------------- */}
      <section className="section section--tight">
        <div className="wrap">
          <div className="release">
            <div>
              <p className="eyebrow eyebrow--muted">Current version</p>
              <p className="release__v">
                {VERSION}
                <span className="release__tag">No tags published yet</span>
              </p>
              <p className="lead" style={{ marginTop: 'var(--s-4)' }}>
                {VERSION} is the workspace version in <code>firmware/Cargo.toml</code>. Releases
                are published as <code>v*</code> tags, and none have been cut yet — so at the
                moment there is nothing on the Releases page to download. The newest build is
                always the <code>quire-x3-images</code> artifact attached to the most recent
                successful CI run on <code>main</code>.
              </p>
            </div>
            <div className="release__actions">
              <a className="btn" href={ACTIONS_URL} target="_blank" rel="noreferrer">
                <DownloadIcon />
                Latest CI images
              </a>
              <a className="btn btn--ghost" href={RELEASES_URL} target="_blank" rel="noreferrer">
                Releases
              </a>
            </div>
          </div>
        </div>
      </section>

      {/* The files ----------------------------------------------------- */}
      <section className="section">
        <div className="wrap">
          <div className="section-head">
            <span className="eyebrow">The artifact</span>
            <h2>What is in quire-x3-images</h2>
            <p>
              The same four files are attached to every CI run and to every <code>v*</code>{' '}
              release. Sizes are the exact byte counts produced by this tree.
            </p>
          </div>

          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th scope="col">File</th>
                  <th scope="col">Flash at</th>
                  <th scope="col">Size</th>
                  <th scope="col">What it is</th>
                </tr>
              </thead>
              <tbody>
                {RELEASE_FILES.map((f) => (
                  <tr key={f.name}>
                    <td>
                      <code className="num">{f.name}</code>
                    </td>
                    <td className="num">{f.flashAt}</td>
                    <td className="num">
                      {fmt.format(f.bytes)} B
                      <br />
                      <span className="muted">{mib(f.bytes)}</span>
                    </td>
                    <td>{f.what}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <h3 style={{ marginTop: 'var(--s-7)' }}>Check what you downloaded</h3>
          <p className="measure">
            Compare the byte counts above with what landed on disk, and keep a checksum of each
            file so a later comparison is one command.
          </p>
          <CodeBlock
            lang="shell"
            code={`# sizes, in bytes
wc -c quire-x3-factory.bin quire-x3.bin quire-recovery.bin quire-assets.bin

# checksums for later
sha256sum quire-*.bin > quire-images.sha256

# and to verify them again afterwards
sha256sum -c quire-images.sha256`}
          />
          <p className="measure muted">
            Releases cut from a <code>v*</code> tag publish their own SHA-256 alongside the
            files; that is the checksum the reader itself verifies when it updates over Wi-Fi.
          </p>

          <h3 style={{ marginTop: 'var(--s-7)' }}>Flashing, once the backup is safe</h3>
          <CodeBlock
            lang="shell"
            code={`# first install: the complete 16 MB image
espflash write-bin 0x0 quire-x3-factory.bin

# or a single partition, once the factory image is on
espflash write-bin 0xa0000  quire-x3.bin
espflash write-bin 0x20000  quire-recovery.bin
espflash write-bin 0xca0000 quire-assets.bin`}
          />
          <p className="measure">
            <Link className="link-arrow" to="/guide#flash">
              The whole flashing walkthrough
            </Link>
          </p>
        </div>
      </section>

      {/* Flash layout -------------------------------------------------- */}
      <section className="section section--tinted">
        <div className="wrap">
          <div className="section-head">
            <span className="eyebrow">Flash layout</span>
            <h2>Where everything lives in the 16 MB</h2>
            <p>
              From <code>firmware/device/partitions.csv</code>. The offsets above are these
              partitions.
            </p>
          </div>
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th scope="col">Partition</th>
                  <th scope="col">Offset</th>
                  <th scope="col">Size</th>
                  <th scope="col">Holds</th>
                </tr>
              </thead>
              <tbody>
                {PARTITIONS.map((p) => (
                  <tr key={p.name}>
                    <td>
                      <code className="num">{p.name}</code>
                    </td>
                    <td className="num">{p.offset}</td>
                    <td className="num">{p.size}</td>
                    <td>{p.holds}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </section>

      {/* Devices ------------------------------------------------------- */}
      <section className="section">
        <div className="wrap">
          <div className="section-head">
            <span className="eyebrow">Supported devices</span>
            <h2>One reader, for now</h2>
          </div>
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th scope="col">Device</th>
                  <th scope="col">Status</th>
                  <th scope="col">Notes</th>
                </tr>
              </thead>
              <tbody>
                {DEVICES.map((d) => (
                  <tr key={d.device}>
                    <td>
                      <strong>{d.device}</strong>
                    </td>
                    <td>
                      <span className={d.status === 'supported' ? 'ok' : 'no'}>
                        {d.status === 'supported' ? 'Supported' : 'Not yet'}
                      </span>
                    </td>
                    <td>{d.note}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="measure muted" style={{ marginTop: 'var(--s-5)' }}>
            Quire is not affiliated with Xteink. Flashing it replaces the firmware your reader
            shipped with, and this project cannot give that firmware back to you — only your own
            backup can.
          </p>
        </div>
      </section>
    </>
  )
}
