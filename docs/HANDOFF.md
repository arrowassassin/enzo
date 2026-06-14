# Enzo — v0.2 continuation handoff

> Read this top-to-bottom before changing code. It tells you exactly what to
> build next, where every piece lives, the conventions you must not break, and
> how to verify your work **without a GPU display** (this matters — you cannot
> open the app window in CI/cloud; you verify with the snapshot harness).

Current state: committed to `main`. The workspace compiles, `clippy --workspace
--all-targets -D warnings` and `fmt --all --check` are clean, and 285 tests pass
(including 4 headless UI snapshot/click tests). Your job is to turn the remaining
**placeholders into real, working features**, pixel-faithful to the mockups.

---

## 0. Golden rules (do not break these)

1. **No placeholders, no demo data in the final state.** Every surface must do
   real work against the real backend. If you add a stub, leave a `// TODO(...)`
   AND a tracking note in this file's "Status" section — but the goal is zero.
2. **Match the mockups exactly.** The source of visual truth is
   `design/mockups/*.html` (open them; they contain exact colors, spacing,
   fonts, icons). The product spec is `design/design-document.md`.
3. **Keep CI green at all times.** Before finishing any task:
   - `~/.cargo/bin/cargo fmt --all`
   - `~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings`
     (the only acceptable remaining warning is the unrelated `block v0.1.6`
     future-incompat note from a transitive metal dep)
   - `~/.cargo/bin/cargo test --workspace`
   The repo has a **pre-commit hook** that runs fmt+clippy+tests and BLOCKS the
   commit if any fail. Don't bypass it.
4. **Never add `Co-Authored-By` trailers** to commits or PRs. (User preference.)
5. **ATP schema is versioned from v0 — additive only.** Add new methods; don't
   break the shape of existing ones. New daemon methods go in
   `crates/enzo-daemon/src/atp/` (core families in `mod.rs`, the extended
   `theme/git/editor/db.schema/db.table/db.tabs` families in `atp/ext.rs`).
6. **`unsafe_code` is `forbid`** workspace-wide. Don't use `unsafe` (this also
   means you can't `std::env::set_var` in tests on edition 2024 — design around
   it).
7. Keep the hot path (render/input) synchronous; tokio is for I/O only
   (design doc §2/§4).

---

## 1. Repo map (what lives where)

```
crates/
  enzo/             orchestrator binary: boots enzo-daemon then enzo-client
  enzo-daemon/      headless state owner + ATP broker (JSON-RPC 2.0 over UDS)
    src/atp/mod.rs    transport + session/db/lsp/browser/prompt/block/display
    src/atp/ext.rs    theme.* git.* editor.* db.schema.* db.table.* db.tabs.*
    src/state.rs      DaemonState: all maps keyed by string id (sessions, db
                      conns, db tabs, lsp, browser pages+procs, themes, prompts)
  enzo-client/      egui/eframe 0.31 (wgpu) UI  ← MOST OF YOUR WORK
    src/gui/mod.rs        EnzoApp (eframe::App), panels, surfaces, channels
    src/gui/theme.rs      palette consts + font install + egui Visuals
    src/gui/terminal_view.rs  terminal grid painter
    src/atp/mod.rs        AtpClient (async UDS JSON-RPC client) + DaemonMessage
    src/surface.rs        per-surface UI state structs (IdeState, DbState, ...)
    src/terminal/mod.rs   VT100 emulator (vte)
    tests/ui_snapshot.rs  egui_kittest headless snapshot+click tests
    tests/snapshots/*.png baselines (committed)
    assets/*.ttf          JetBrains Mono, Silkscreen, Tabler icons
  enzo-db/          SQLite driver, Arrow results, schema introspect, table
                    view/edit, pagination, query tabs  (all real, all tested)
  enzo-editor/      ropey buffer, tree-sitter highlight, lang registry, fmt, LSP, DAP
  enzo-git/         git2: status/diff/stage/commit/push/branches/worktrees
  enzo-theme/       Theme model + 6 built-in themes (TOML), Theme::builtin(id)
  enzo-browser/     CDP client + headless Chrome launcher (launch.rs)
  enzo-adapters/    enzo-claude AI-CLI wrapper binary (PTY proxy → prompt.* ATP)
  enzo-vault/       envelope-encrypted secret store (Argon2id + AEAD)
  enzo-redact/      secret scrubbing for agent context
design/
  design-document.md      product spec (v0.2)
  mockups/*.html          PIXEL TRUTH — open these
```

### Build / run / test
```
~/.cargo/bin/cargo build --bins          # all binaries → target/debug/
./target/debug/enzo                      # orchestrator: daemon + GPU window
                                         # (GUI; needs a display — not in CI)
RUST_LOG=debug cargo run -p enzo-daemon  # daemon alone (terminal 1)
RUST_LOG=debug cargo run -p enzo-client  # client alone (terminal 2)
```
Stale daemon? `pkill enzo-daemon; rm -f /tmp/enzo-atp.sock`.

---

## 2. How to verify UI work WITHOUT a display (read this twice)

You cannot open the wgpu window in the cloud. You verify with **egui_kittest**,
which renders the real `EnzoApp` headlessly (Metal/Vulkan offscreen), clicks it,
and writes PNG snapshots.

```
# run the UI tests (compares against committed baselines):
~/.cargo/bin/cargo test -p enzo-client --test ui_snapshot
# after an intentional visual change, regenerate baselines:
UPDATE_SNAPSHOTS=1 ~/.cargo/bin/cargo test -p enzo-client --test ui_snapshot
```

Snapshots land in `crates/enzo-client/tests/snapshots/`. **Open the PNGs and
compare to the mockups.** The harness is deterministic: it builds the app via
`EnzoApp::__new_app_for_test_offline(cc)` (no daemon connection, steady caret),
so snapshots don't depend on whether a daemon is running.

To get pixel-exact mockup targets to compare against, render the HTML mockups:
```
cd design/mockups && python3 -m http.server 8799 &   # then use a headless
# browser / Playwright to screenshot http://localhost:8799/<name>.html
```

**Workflow for every UI change:** make the change → add/adjust a kittest test
that exercises it → `UPDATE_SNAPSHOTS=1` → open the PNG → confirm it matches the
mockup → run without the env var to confirm it's stable. Add new snapshot tests
for every new interactive element so regressions are caught.

---

## 3. The async UI ↔ daemon pattern (you will reuse this constantly)

The client talks to the daemon on a background tokio thread; the egui UI thread
never blocks. The channel wiring in `gui/mod.rs`:

- `enum UiCommand { ... }` — UI → daemon requests (fire-and-forget from the UI).
- `enum Incoming { ... }` — daemon → UI results, drained each frame in
  `EnzoApp::drain_incoming(ctx)`.
- `run_atp(sock, ctx, tx: Sender<Incoming>, cmd_rx)` — the background task. It
  owns the `AtpClient`, matches each `UiCommand`, calls the async client method,
  and for anything that returns data, sends an `Incoming::*` back and calls
  `ctx.request_repaint()`.

**The browser screenshot stream is your reference implementation.** Study it end
to end and copy the shape for DB results:
- `UiCommand::BrowserShot { id }` → `client.browser_screenshot(id).await` →
  decode → `Incoming::BrowserFrame(ColorImage)` → `drain_incoming` loads a
  texture. (`browser_launch`, `browser_input`, `browser_navigate` follow the
  same pattern.)

`AtpClient` (in `src/atp/mod.rs`) exposes `request(method, params) -> Value` plus
typed helpers. Add typed helpers there for the DB methods you need.

---

## 4. TASKS (in priority order). Each has: where, how, acceptance, verify.

### TASK 1 — Database surface: real backend (NO demo data)  ★ do first

**Problem:** `DbState::demo()` (in `crates/enzo-client/src/surface.rs`) hardcodes
connections, tables, columns, and rows. "Add connection" fakes a name. RUN does
nothing. Schema is a static list.

**The backend already exists and is tested** — wire the UI to it. Daemon methods
(see `enzo-daemon/src/atp/ext.rs` + `mod.rs`, and `enzo-db`):
- `db.connect { id, path }` → `{ driver }`   (path can be a file or `:memory:`)
- `db.query { conn, sql }` → `{ columns: [..], rows: [[..]] }`
- `db.execute { conn, sql }` → `{ affected }`
- `db.schema.tables { conn }` → `{ tables: [{name, kind}] }`
- `db.schema.columns { conn, table }`, `db.schema.indexes { conn, table }`
- `db.table.browse { conn, table, page, size }` → `{ columns, rows, total, page, size }`
- `db.table.update/insert/delete` (injection-safe; pk-anchored)
- `db.tabs.list/open/close/rename/set_sql { conn, ... }`

**Do:**
1. Add typed `AtpClient` helpers in `crates/enzo-client/src/atp/mod.rs`:
   `db_connect`, `db_query`, `db_schema_tables`, `db_table_browse`, etc.
   (mirror `browser_*`). Parse `{columns, rows}` into `Vec<String>` + `Vec<Vec<String>>`.
2. Add `UiCommand` variants: `DbConnect{conn,path}`, `DbQuery{conn,sql,tab}`,
   `DbLoadTables{conn}`, `DbBrowseTable{conn,table}`. Handle them in `run_atp`.
3. Add `Incoming` variants: `DbConnected{conn,driver}`, `DbTables{conn,tables}`,
   `DbResult{columns,rows,ms?,error?}`. Handle in `drain_incoming` → update
   `DbState`.
4. Rewrite `DbState` (in `surface.rs`): remove `demo()`'s fake rows/tables.
   Keep `tabs`, `active_tab`, `connections`, `active_conn_idx`. Add per-connection
   real `tables: Vec<TableInfo>` populated from `db.schema.tables`. Results
   (`columns`/`rows`/`query_ms`/`error`) come from `db.query`.
5. **Connection dialog (real):** the sidebar "+ add connection" must open a modal
   (like `draw_settings`) with a **connection-string / file-path field** (start
   with SQLite paths; the daemon's `AnyPool::sqlite` takes a path or `:memory:`).
   On submit → `UiCommand::DbConnect` → on `DbConnected`, fetch tables. Default
   the first connection to a real on-disk SQLite file (e.g. create/seed a demo
   `~/.enzo/demo.db` on first run so the surface isn't empty, but it must be a
   REAL queryable db, not in-memory faked rows).
6. **RUN (⌘↵):** execute the active tab's SQL via `db.query` against the active
   connection; render real `columns`/`rows` in the grid; show real timing and
   real errors (red) from the daemon.
7. **Schema click:** clicking a table should browse it — populate the CURRENT
   query tab with `SELECT * FROM <table> LIMIT 100;` and run it (don't always
   spawn a new tab — that was a reported bug). Use `db.table.browse` for paging.
8. **Pagination:** the grid must lazily page large tables via `db.table.browse`
   `{page,size}` using the returned `total` for the scrollbar (design doc §5.4 —
   "millions of rows"). Don't load everything.

**Acceptance:** With a real SQLite file, you can: add a connection by path, see
its real tables in the sidebar, type SQL + RUN and get real rows, click a table
to browse it, page through a large table, and see a real SQL error rendered. No
hardcoded row data anywhere.

**Verify:** add kittest snapshot tests: a connected DB surface showing real
results, the connection dialog open, an error state. Compare to
`design/mockups/database.html` and `db-connection.html`. Also add a daemon
integration test (in `crates/enzo-daemon/tests/atp_integration.rs`) that does
connect→execute(create+insert)→query and asserts the rows — extend the existing
ones.

---

### TASK 2 — Theme selection actually re-themes the app  ★

**Problem:** `Settings` lists 6 themes but selecting one does nothing. The egui
palette is hardcoded as `pub const`s in `gui/theme.rs`.

**Do:**
1. Add `enzo-theme` as a dependency of `enzo-client` (`Cargo.toml`).
2. Refactor `gui/theme.rs` so colors are **runtime-resolved**, not consts.
   Introduce a `Palette` struct holding every role color (`bg_page`, `bg_surface`,
   `bg_dock`, `bg_side`, `bg_bar`, `bg_card`, `border`, `fg0/1/2`, `muted`,
   `faint`, `teal/accent`, `purple*`, `green*`, `blue*`, `amber*`, `red*`,
   `keyword`, `term_fg`). Build a `Palette` from an `enzo_theme::Theme` by reading
   its `roles`/`syntax` maps (`Theme::role("background")`, etc. → `parse_hex`).
   Keep the current Enzo-Dark values as the default `Palette`.
3. Store the active `Palette` in `EnzoApp` and thread it (or a `&Palette`) into
   the draw helpers, OR keep a process-global `ArcSwap<Palette>` that `theme::*`
   accessors read. Either way, **all** `theme::BG_SURFACE`-style references must
   resolve from the active palette so a theme switch repaints everything,
   including custom-painted bits (terminal grid, cursor, tree hovers).
4. `install_theme(ctx, &Palette)` maps the palette → egui `Visuals` (as `visuals()`
   does today) and calls `ctx.set_style(...)`. Call it on selection change in
   `draw_settings` (map UI names → ids: "Enzo Dark"→`enzo-dark`, "Tokyo
   Night"→`tokyo-night`, "Matrix"→`matrix`, "Game Boy DMG"→`gameboy-dmg`,
   "Amber CRT"→`amber-crt`, "Enzo Light"→`enzo-light`).
5. Effects toggles (scanlines/phosphor/CRT) can stay non-functional for now but
   wire the data (`Theme::effects`) — mark clearly as not-yet-rendered.

**Acceptance:** picking Matrix turns the whole UI green-on-black; Enzo Light
makes it light; etc. — every panel, the terminal text, and custom-painted
elements update live.

**Verify:** kittest snapshot per theme (`settings_matrix.png`, etc.); confirm the
whole frame's palette changed, not just widget chrome.

---

### TASK 3 — Terminal semantic blocks (OSC 133)  per mockups/terminal.html

**Problem:** the terminal is a raw VT grid. The mockup shows **command blocks**:
each command + its output + exit status + duration, with a colored left border
(green = success, red = fail), and AI/ATP blocks composited in the same column.

**Do:**
1. In `enzo-daemon` (PTY/scrollback), parse **OSC 133** marks (A=prompt start,
   B=command start, C=output start, D=command end+exit code) and **OSC 7** (cwd).
   Emit block boundaries to the client (new ATP notification, e.g.
   `session.block { id, kind, command, exit, duration_ms, cwd }`), or annotate
   the output stream so the client can segment it. Keep raw bytes flowing too
   (compat plane). See design doc §5.1 / §7 (ATP layers 0–1).
2. In `enzo-client`, render blocks in the content column: a left border (green/
   red/purple), the command line, collapsible output, a Silkscreen status chip
   ("✓ 88 PASS · 2.1s"). AI/agent `block.push` cards already render via the
   overlay — composite them inline in the same column per the mockup.
3. Keep the existing raw-grid rendering as the fallback for full-screen TUI apps
   (vim/tmux) — those take over a block in "raw" mode (design doc §3.2).

**Acceptance:** running `cargo test` in the terminal shows a bordered block with
an exit chip, matching `mockups/terminal.html`. vim still works full-screen.

**Verify:** snapshot a synthesized block sequence; compare to the mockup.

---

### TASK 4 — IDE: wire live syntax + LSP into the editor view

The editor view currently shows file text with a tiny hand-rolled tinter.
`enzo-editor` already has real tree-sitter highlighting (`editor.highlight`
returns spans) and an LSP client. Wire the open file through `editor.highlight`
for real spans, show the line-number gutter + breakpoint column + inline
diagnostics card per `mockups/ide.html`, and surface the debugger/variables/
call-stack panel. Make the file actually editable (it's a viewer now) backed by
`enzo_editor::Buffer` (ropey).

---

### TASK 5 — The four product pillars (design doc §6/§8/§9/§14)

Only after 1–4 feel enterprise-solid. Build order:
1. **Workspaces** (`workspace.*`): project tabs on the menu bar; each project owns
   its own IDE/terminal-tabs/DB+query-tabs/browser-tabs + git worktree; snapshot &
   resume; lazy surface activation + hibernation. The daemon already keys every
   resource by string id — add a `workspace_id` scope layer (additive). See §6.
2. **Unified search** (`search.*`): ⌘K global fan-out across files/symbols/
   scrollback/DB schema/DOM/refs/commands, plus ⌘F scoped per-surface fast search.
   §8.
3. **Reference graph** (`ref.*`): the keystone — typed `Ref`s (frozen snapshot +
   live locator), `⌘E` to grab on any surface, drop anywhere (DB row → terminal,
   DOM/console → editor, etc.), secrets scrubbed via `enzo-redact`. §9.
4. **Auth / load screen** (§14): master password (enzo-vault: Argon2id + AEAD) +
   Touch ID / Windows Hello as a second unlock path; gate the workspace behind it.

---

## 5. Visual fidelity checklist (against the mockups)

- Fonts already embedded: **Silkscreen** (8px UPPERCASE labels/badges/status),
  **JetBrains Mono** (body/code), **Tabler** icons (dock/sidebar). Use
  `theme::pixel(8.0)` for labels and `theme::icon_font(...)`/`ICON_*` for icons.
- Palette is the exact mockup hex (see `theme.rs`): page `#0e0c14`, surface
  `#16131f`, dock `#120f1a`, sidebar `#1a1626`, bars `#221d30`, border `#3a3450`,
  teal `#5dcaa5`, purple `#534ab7/#7f77dd/#afa9ec`, green `#639922/#97c459`.
- Chunky 2–3px borders, gently rounded corners (egui `CornerRadius`, integers in
  0.31), pixel-sharp.
- When in doubt, open the mockup HTML and read the inline styles.

---

## 6. Status / running notes (update this as you go)

- [ ] Task 1 — DB real backend + connection dialog
- [ ] Task 2 — live theme switching
- [ ] Task 3 — OSC 133 terminal blocks
- [ ] Task 4 — IDE live syntax/LSP + editable buffer
- [ ] Task 5 — workspaces / search / refs / auth

Keep every step green (fmt + clippy + tests), regenerate snapshots for visual
changes, and commit in coherent chunks with clear messages (no Co-Authored-By).
