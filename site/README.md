# The Quire website

The public site for Quire, published to <https://arrowassassin.github.io/quire/>.

Vite + React 19 + TypeScript, no UI framework: the palette, type scale and
spacing live in `src/styles/tokens.css` and everything else reuses them.

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
| `src/pages/` | Home, Features, Screens, Guide, Downloads, FAQ, NotFound. |
| `src/components/` | Nav, Footer, DeviceFrame, Lightbox, CodeBlock, Callout, Reveal, Icons. |
| `src/lib/` | `asset.ts` (BASE_URL-aware paths), `catalogue.ts`, `theme.ts`, `useSeo.ts`, `useReveal.ts`, `useScrollSpy.ts`. |
| `src/data/site.ts` | Every fact and number shown on the site, in one place. |
| `src/data/screens.ts` | The screenshot catalogue: one entry per PNG in `public/shots`. |
| `src/data/shots.ts` | The file-name list, used as a fallback when `screens.ts` is empty. |
| `src/data/captions.ts` | Fallback captions, used only where `screens.ts` has no entry. |
| `public/shots/` | The real 528 × 792 screenshots rendered by the simulator. |
| `src/assets/fonts/` | Literata, Atkinson Hyperlegible and JetBrains Mono, subset to Latin and converted to WOFF2 from `firmware/crates/quire-fonts/ttf/`. |

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
