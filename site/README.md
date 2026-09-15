# The Quire website

The public site for Quire, published to <https://arrowassassin.github.io/quire/>.

Vite + React 19 + TypeScript, no UI framework: the palette, type scale and
spacing live in `src/styles/tokens.css` and everything else reuses them.

One runtime dependency beyond React: **`esptool-js`**, pinned to `0.6.1`, which
the `/install` page uses to talk to an Xteink X3 over the Web Serial API. It is
loaded with a dynamic `import()` inside that page, so no other route pays for
it — `npm run build` puts it in its own chunk. `@types/w3c-web-serial` supplies
the `navigator.serial` types and is listed in `tsconfig.json`'s `types`.

## Running it

```sh
cd site
npm install
npm run dev        # http://localhost:5173/quire/
```

```sh
npm run build      # tsc --noEmit, vite build, then scripts/postbuild.mjs
npm run preview    # serves dist/ at http://localhost:4173/quire/
npm run typecheck  # tsc --noEmit on its own
```

`npm run build` must be clean: TypeScript runs in strict mode with
`noUnusedLocals`, `noUnusedParameters` and `noUncheckedIndexedAccess` on.

## How it is laid out

| Path | What |
|---|---|
| `src/styles/tokens.css` | The one token layer: `@font-face`, palette (light and dark), type scale, spacing, motion. |
| `src/styles/{base,layout,components,pages}.css` | Element defaults, page frame, components, per-page blocks. |
| `src/pages/` | Home, Features, Screens, Install, Guide, Downloads, FAQ, NotFound. |
| `src/components/` | Nav, Footer, DeviceFrame, Lightbox, CodeBlock, Callout, Reveal, Icons. |
| `src/lib/` | `asset.ts` (BASE_URL-aware paths), `catalogue.ts`, `theme.ts`, `useSeo.ts`, `useReveal.ts`, `useScrollSpy.ts`, `flash.ts`, `release.ts`. |
| `src/data/site.ts` | Every fact and number shown on the site, in one place. |
| `src/data/screens.ts` | The screenshot catalogue: one entry per PNG in `public/shots`. |
| `src/data/shots.ts` | The file-name list, used as a fallback when `screens.ts` is empty. |
| `src/data/captions.ts` | Fallback captions, used only where `screens.ts` has no entry. |
| `public/shots/` | The real 528 × 792 screenshots rendered by the simulator. |
| `src/assets/fonts/` | Literata, Atkinson Hyperlegible and JetBrains Mono, subset to Latin and converted to WOFF2 from `firmware/crates/quire-fonts/ttf/`. |

### The browser installer (`/install`)

`src/pages/Install.tsx` is the one page with real machinery behind it. It drives
an ESP32-C3 over Web Serial: connect, read the whole 16 MB flash to a file, check
and write a Quire image, and write a saved backup back again.

| Piece | Where |
|---|---|
| The page, its state machine and every string shown | `src/pages/Install.tsx` |
| Formatting, SHA-256, the download, image validation, progress maths | `src/lib/flash.ts` |
| Looking up and streaming a release asset from GitHub | `src/lib/release.ts` |
| Styles (`.stepx`, `.bar`, `.note`, `.pick`, `.confirm`, `.unsupported`, `.restore`) | `src/styles/pages.css` |

Rules this page follows, on top of the site-wide ones:

- **esptool-js is imported dynamically**, inside the connect handler. Never add a
  top-level `import` of it: that would put a megabyte of flasher into the bundle
  every visitor downloads.
- **Nothing writes to the device without a click that says it writes.** Step 2
  only reads. Step 4 needs a ticked confirmation as well as a press.
- **Every failure path calls `transport.disconnect()`** and clears the loader, so
  the port is released and the visitor can start again without reloading. The one
  exception is a short read, where the transport is still healthy and the step
  offers the read again.
- **No progress bar claims a success it has not checked.** A backup is only "done"
  once the read is exactly 16,777,216 bytes *and* the file has been handed to the
  browser; an image is only accepted at exactly that length with `0xE9` as its
  first byte.
- **The unsupported-browser branch renders instead of the steps**, never disabled
  buttons. Headless Chromium has no Web Serial, so this is the branch any
  screenshot test sees.
- **No stubs or mocks in the page.** To exercise the connected states in a test,
  stub `navigator.serial` from the test with `page.addInitScript`.
- There are no published releases yet, so `probeLatestRelease()` normally answers
  `none`; the page says so and points at the CI artifact rather than offering a
  download that cannot work.

The page has not been run against a physical X3 — this project has none — and the
page says so in its own words near the top. Keep that note there.

### Adding or changing screenshots

Drop the PNG in `public/shots/`, add an entry to `src/data/screens.ts`, and
regenerate the fallback list:

```sh
ls public/shots | sed 's/\.png$//'
```

Nothing else needs touching: the gallery, the alt text and the feature strips
all read the catalogue.

### The rules the code follows

- **Every asset path goes through `import.meta.env.BASE_URL`** (`src/lib/asset.ts`),
  because the site is served from `/quire/`, not from the root. A path written as
  `/shots/x.png` will 404 in production.
- **Facts come from `src/data/site.ts` or the facts sheet.** No invented versions,
  dates, download counts or benchmarks.
- **Screenshots are black-on-white 1-bit PNGs.** In dark mode the frame around them
  turns paper-coloured; the images are never inverted.
- **Motion is one `IntersectionObserver`** (`useReveal`) and respects
  `prefers-reduced-motion`.
- **In-page `#hash` links are scrolled to by `ScrollToTop`**, because React Router
  does not do it. The Guide's terminal block opens itself when the hash points
  inside it.

## Deployment

`.github/workflows/pages.yml` builds and deploys on every push to `main` that
touches `site/**` (or the workflow itself), and on `workflow_dispatch`. It runs
Node 22, `npm ci` and `npm run build` in `site/`, then uploads `site/dist` with
`actions/upload-pages-artifact` and publishes it with `actions/deploy-pages`.

GitHub Pages serves static files only, so `scripts/postbuild.mjs` copies
`dist/index.html` to `dist/404.html` (the standard SPA fallback for
`BrowserRouter`) and writes `dist/.nojekyll`.

Repository settings must have **Pages → Build and deployment → Source** set to
**GitHub Actions** for the workflow to be allowed to deploy.

`package-lock.json` is committed and `npm ci` depends on it; `site/node_modules/`
and `site/dist/` are ignored.
