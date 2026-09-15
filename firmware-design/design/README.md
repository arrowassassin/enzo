# Claude Design canvas — Quire firmware

Source files pulled from the Claude Design project
[1ea92246-9758-4312-822b-1fbdf76cbe0e](https://claude.ai/design/p/1ea92246-9758-4312-822b-1fbdf76cbe0e).

| File | Contents |
|---|---|
| `Batch 1 - Foundation.dc.html` | typography, components, signature boards |
| `Batch 2 - Reading Core.dc.html` | reading view, compass, home, sleep, power, locked |
| `Batch 3 - Getting Books.dc.html` | drop, wi-fi, OPDS, calibre, sync, bookshop, phone window |
| `Batch 4 - Library Analytics System.dc.html` | library, book info, folders, end of book, settings, OTA, analytics 60a–e, year in review |
| `Batch 5 - Apps Games First Run.dc.html` | boot, first run, apps, games, developer, recovery, refresh map |
| `support.js` | the `dc-runtime` bundle the `.dc.html` files load (generated — do not edit) |
| `github.md` | the project's sync manifest: screen map and sync history |

Each `.dc.html` is a standalone page: open it in a browser and the canvas renders.
Every artboard is 528 × 792, 1-bit ink on paper.

The project also holds 1:1 and 1-bit PNG exports of every artboard under
`exports/batch1..5/`. Those are generated artifacts and are not mirrored here —
the `.dc.html` sources above are the source of truth. Pull specific PNGs from the
canvas if a dithering check needs them.
