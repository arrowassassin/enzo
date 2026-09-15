import { Link } from 'react-router-dom'
import { ACTIONS_URL, REPO, RELEASES_URL } from '../data/site'

export function Footer() {
  return (
    <footer className="footer">
      <div className="wrap">
        <div className="footer__grid">
          <div>
            <h2>Quire</h2>
            <p style={{ color: 'var(--ink-2)', maxWidth: '24rem' }}>
              Open-source e-reader firmware for the Xteink X3, written in Rust from scratch.
              A printed object, not a device.
            </p>
          </div>
          <div>
            <h2>Site</h2>
            <ul>
              <li><Link to="/features">Features</Link></li>
              <li><Link to="/screens">Screens</Link></li>
              <li><Link to="/guide">Guide</Link></li>
              <li><Link to="/downloads">Downloads</Link></li>
              <li><Link to="/faq">FAQ</Link></li>
            </ul>
          </div>
          <div>
            <h2>Source</h2>
            <ul>
              <li><a href={REPO} target="_blank" rel="noreferrer">Repository</a></li>
              <li><a href={`${REPO}/tree/main/firmware`} target="_blank" rel="noreferrer">Firmware</a></li>
              <li><a href={`${REPO}/tree/main/firmware-design`} target="_blank" rel="noreferrer">Design package</a></li>
              <li><a href={`${REPO}/tree/main/sleep-packs`} target="_blank" rel="noreferrer">Sleep packs</a></li>
            </ul>
          </div>
          <div>
            <h2>Builds</h2>
            <ul>
              <li><a href={RELEASES_URL} target="_blank" rel="noreferrer">Releases</a></li>
              <li><a href={ACTIONS_URL} target="_blank" rel="noreferrer">CI artifacts</a></li>
              <li><a href={`${REPO}/blob/main/LICENSE-MIT`} target="_blank" rel="noreferrer">MIT licence</a></li>
              <li><a href={`${REPO}/blob/main/LICENSE-APACHE`} target="_blank" rel="noreferrer">Apache-2.0 licence</a></li>
            </ul>
          </div>
        </div>

        <div className="footer__fine">
          <p>
            Quire is an independent project. It is not affiliated with, endorsed by or supported
            by Xteink. Dual-licensed MIT or Apache-2.0. Bundled fonts under the SIL Open Font
            License; the dictionary derives from WordNet 3.1 under its own licence; the sleep
            packs are CC0.
          </p>
          <p>Typeset in Literata, Atkinson Hyperlegible and JetBrains Mono — the firmware’s own faces.</p>
        </div>
      </div>
    </footer>
  )
}
