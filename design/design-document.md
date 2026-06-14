# Enzo — Design Document

> The AI-native developer workspace. One app that holds your whole project —
> terminal, editor, database, and browser — behind one login, sharing one AI brain
> and one memory. 8-bit aesthetic, modern GPU engine. Open source, $0 to ship.

Version: 0.2 (draft) · Last updated: 2026-06-13

---

## 1. Vision

Enzo is not "another terminal emulator." It is a **GPU application platform whose
unit of work is a project workspace**, and which folds the four tools a developer
lives in — **terminal, code editor (IDE), web browser, and database client** —
into one app, unified by a single thing nobody else has: **one AI agent with
simultaneous, structured context across all four surfaces.**

The reframing that makes everything else possible: the moment Enzo gained a GPU
*semantic plane* (rich UI beyond the 80×24 character grid), it stopped being a
terminal and became an IDE-class application that *contains* a terminal. A browser
panel, an editor panel, and a DB panel are then just more panels on the same
compositor.

**The product unit is the project workspace, not the window.** You load a project
(from the file explorer); it becomes a tab on the menu bar. Each loaded project
gets its own self-contained workspace — an IDE, one or more terminal tabs, a
database client with its own query tabs, and a browser with its own tabs — plus
its own managed git worktree. Switch projects the way you switch browser tabs;
each one resumes exactly where you left it. Close the laptop, reopen tomorrow,
and every terminal, open file, DB connection, browser tab, and AI thread is
still there.

### The one reason to unify
"VS Code + Chrome + DBeaver + iTerm" already exists and is free. The only
justification for building Enzo is the **cross-surface agent loop**: the agent can
read the failing `GET /api/me` 401 in the browser's network tab, jump to the auth
handler in the editor, query the `users` table to check the row, and propose the
fix — in one instruction, with all three as structured context. No combination of
separate tools can hand an AI that unified view. If that loop isn't magic, Enzo
shouldn't be built.

**The thesis, stated plainly:** in an AI-first world, the developer should never
need to leave one app to get their work done. Enzo is that app.

---

## 2. Product strategy (the PM view)

A great architecture does not sell itself. This section is the honest case for who
buys Enzo, why, what already works, and what is still missing to be a *complete*
product.

### 2.1 Who it's for
Full-stack and backend developers who live across the terminal, an editor, a
database, and a browser every hour — and who now also run an AI coding agent
(Claude Code, Cursor, Codex, Qwen) in that loop. Their daily tax is **context
switching**: copy-pasting a stack trace from the terminal into the editor, a row
from the DB GUI into a comment, a failing request from DevTools into the AI chat.
Every hop loses context and breaks flow.

### 2.2 The one-line pitch
*"The AI-native workspace where your terminal, editor, database, and browser share
one brain — and one memory."*

### 2.3 Why now
AI agents made the terminal the new IDE, but **agents are blind across tools**.
They see the files you give them and nothing else. Enzo gives the agent eyes on
all four surfaces and a typed way to reference anything in any of them. The timing
is the product: the agent loop only became valuable once agents got good.

### 2.4 Go-to-market: land as a terminal, expand to a workspace
The biggest adoption risk is asking developers to switch four tools at once. So we
don't. **Day-one value is being the best AI terminal** — works with any CLI agent,
zero agent buy-in, drop-in for iTerm/Warp. The IDE, DB, and browser surfaces
appear *on demand* (progressive disclosure). Land as a terminal; expand into the
full workspace as the user pulls features toward them. This is the wedge.

### 2.5 The moat
Single-tool competitors each own one surface — Warp/iTerm (terminal),
Cursor/Zed/VS Code (editor), DBeaver/TablePlus (DB), Chrome (browser). None can
replicate the **unified cross-surface context** without rebuilding the other three.
The moat is not any single surface; it is **the reference graph + persistent
project workspaces that bind them**. Bundling is the defensibility.

### 2.6 What sells (developer-resonant, in priority order)
1. **Resume exactly where you left off.** Project-workspace snapshots restore every
   terminal, open file, DB connection, browser tab, and AI thread. The daemon/
   client split makes this native, not a hack.
2. **One AI that sees everything.** The demo that closes the sale: drag a 401 from
   the network tab → the agent reads the auth handler and the `users` row →
   proposes the fix. One instruction.
3. **It's fast.** GPU compositor, sub-frame keystroke echo. Latency is a feature.
4. **It's trustworthy.** Local-first; secrets scrubbed from all agent context;
   sandboxed browser; everything behind a real auth gate. Enterprise-credible.
5. **No lock-in.** Works with the AI CLI you already use; ATP is an open spec.
6. **$0, open source, single binary, memory-safe (Rust).**

### 2.7 What needs improvement / honest risks
- **Per-surface long tail.** Each surface has a brutal feature tail (Chrome
  DevTools, VS Code, DBeaver are decades deep). Mitigation: target *90% of the
  daily loop*, never parity; "just shell out to the real tool" is always allowed.
- **Resource cost of N workspaces × 4 surfaces.** Multiplying projects multiplies
  PTYs, LSP/DAP processes, DB connections, and browser hosts. Mitigation:
  **lazy surface activation** (a surface spins up only when first opened),
  **project hibernation** (idle workspaces release heavy resources, keep state),
  and per-workspace resource budgets.
- **Onboarding overwhelm.** A tool "loaded with features" can read as cluttered.
  Progressive disclosure must be *real*: a clean terminal until you ask for more.
- **ATP adoption.** The structured-agent layer only pays off if agents speak it.
  Mitigation: Layers 0–1 work with any agent; we ship the adapters ourselves
  (Claude Code adapter is built) and publish the spec.
- **Positioning clarity.** Marketing must lead with the *loop*, not a feature list,
  or Enzo reads as "yet another AI editor."

### 2.8 Completeness checklist — "the dev never leaves the app"
To be a product a developer can live in, Enzo needs all of:
- [x] Best-in-class terminal + AI CLI wrapper (ATP prompt/diff cards)
- [x] IDE intelligence: tree-sitter, LSP, DAP, formatters, language registry
- [x] Database client: schema browser, table view/edit, query tabs, pagination
- [x] Browser surface via CDP (devtools panels)
- [x] Git: status, diff, stage, commit, push/pull, branches, **worktrees**
- [x] Themes + effects, hot-reload
- [ ] **Project workspaces**: project tabs, per-project 4-surface set, snapshots
- [ ] **Unified search + command palette** (and per-surface fast search)
- [ ] **Cross-surface reference graph** (`ref.*`) — the keystone
- [ ] **Auth/load screen** with master password + Touch ID / Windows Hello
- [ ] Per-project AI thread history + scoped agent memory
- [ ] Settings sync + workspace sync across machines (daemon makes it natural)
- [ ] Real-time collaboration (multi-client on one session — already in the model)
- [ ] Theme/plugin registry; onboarding/first-run; full a11y; telemetry-free stance

Sections 6, 8, 9, and 14 specify the four unchecked pillars.

---

## 3. Non-negotiable principles

1. **Orchestrate engines, don't author them.** Never write a browser engine, a
   language analyzer, a debugger, or a DB driver. Embed mature engines and speak
   standard protocols. The day someone proposes writing our own renderer-from-
   scratch or SQL parser, the project has lost.
2. **The agent loop is the product.** A feature that doesn't improve the cross-
   surface AI loop is deferrable — "just shell out to the real tool."
3. **Compatibility is sacred.** The terminal must run every existing CLI/TUI app
   (vim, tmux, ssh, htop) perfectly. Breaking them is never acceptable.
4. **Latency is a feature.** Sub-frame keystroke echo; nothing blocks input.
5. **Open source, $0, memory-safe.** Permissive licenses only; Rust core;
   sandboxes around anything that touches untrusted input.
6. **Accessibility is UX, not an add-on.**
7. **The workspace is the unit; state is sacred.** Closing the renderer, switching
   projects, or roaming networks never loses work. Every workspace is resumable.
8. **Progressive disclosure beats feature density.** Clean terminal first; IDE/DB/
   browser/search/refs appear when summoned. Power without clutter.

---

## 4. Architecture

### 4.1 Daemon + client split
The single most important structural decision. A headless **session daemon** owns
all state (PTYs, scrollback, DB connections, LSP/DAP processes, the credential
vault, browser hosts, the reference store, ATP connections). A disposable **GPU UI
client** renders it.

This one move buys: crash resilience (renderer panic loses nothing), persistent &
detachable sessions, mosh-style network roaming, multiple windows over one
session, headless/CI use — and, crucially, **project-workspace snapshots** that
restore an entire project's state on demand.

```
+-----------------------------------------------------------+
|                     ENZO UI CLIENT                        |
|  Load screen (Touch ID / Windows Hello) → workspace tabs  |
|  GPU window (wgpu: Metal / DX12 / Vulkan / WebGPU)        |
|                                                           |
|  Project tabs: [api-server*] [web-app] [data-pipeline] [+]|
|  +----------------+      +---------------------------+    |
|  |  COMPAT PLANE  |      |     SEMANTIC PLANE        |    |
|  |  xterm grid    |      | blocks, diffs, widgets,   |    |
|  |  (raw VT)      |      | editor, browser, db, agent|    |
|  +--------+-------+      +-------------+-------------+    |
|           +---- GPU compositor --------+                  |
|  Unified search (⌘K) · command palette · refs tray       |
|  AccessKit a11y tree -> UIA / AT-SPI / NSAccessibility    |
+---------------------------+-------------------------------+
                            |  local IPC (UDS / named pipe)
                            |  or QUIC when remote
+---------------------------+-------------------------------+
|                  ENZO SESSION DAEMON                      |
|  Workspace registry (per-project scope of all resources) |
|  PTY mgr . Scrollback . Block model + OSC 133/7/8 parser  |
|  ATP broker (JSON-RPC) . Reference store (cross-surface)  |
|  Search index (files, scrollback, schema, DOM)           |
|  Credential vault (envelope-encrypted)                   |
|  LSP/DAP host . DB connections (ADBC) . WASM plugins     |
|  Git host (status/diff/commit/push/worktrees)            |
|  CEF browser host (SANDBOXED, no vault access)           |
+----+--------------------+--------------------+-----------+
     |                    |                    |
  your shells         Claude Code          DB engines /
  (per workspace)     (or any agent,        Postgres /
                       speaks ATP)          DuckDB / ...
```

### 4.2 Dual-plane rendering
- **Compatibility plane** — a bulletproof xterm/VT emulator. Runs any app.
- **Semantic plane** — GPU-composited rich UI: blocks, diffs, the editor, the
  browser texture, the data grid, agent UI, prompt cards. A legacy full-screen app
  simply takes over a block in "raw" mode.

### 4.3 Workspace scoping
Every resource in the daemon is keyed by a string id. A **workspace** is a scope
over those ids: a project has a `workspace_id`, and its sessions, DB connections,
LSP/DAP clients, browser pages, query tabs, and refs are namespaced under it.
`workspace.*` ATP methods create/list/close/snapshot a workspace; resource-
creating methods (e.g. `session.spawn`, `db.connect`) carry a `workspace` field.
This is additive over the existing flat model — no rewrite, just a scope layer.

---

## 5. Technology stack (all MIT / Apache-2.0 / BSD — $0)

Revalidated 2026-06-13. Each row carries a verdict; see §5.1 for the reversibility
map and the rationale behind the adjusted choices.

| Layer | Technology | Verdict | Notes |
|---|---|---|---|
| Core language | **Rust** | ✅ keep | Memory-safe, single binary, the ecosystem already exists here |
| GPU | **wgpu** (WGSL shaders) | ✅ keep, benchmark | One abstraction over Metal/DX12/Vulkan/WebGPU + future WebGPU client; keep behind a thin renderer trait |
| UI compositor | **Own**, on wgpu + cosmic-text + taffy | 🔴 build own | Prototype on GPUI to learn, but do NOT make it load-bearing |
| 2D vector / effects | hand-written **WGSL** passes; **vello** optional | 🟡 demote vello | 8-bit/CRT effects don't need a full vector engine |
| Text shaping | **cosmic-text** + **swash** (+ rustybuzz) | ✅ keep | Never hand-roll |
| Layout | **taffy** | ✅ keep | Flexbox math |
| Windowing/input | **winit** | ✅ keep | |
| Accessibility | **accesskit** | ✅ keep | UIA / AT-SPI / NSAccessibility — high value |
| Terminal core | **vte** + **alacritty_terminal** + **portable-pty** | ✅ keep, +grid | Use Alacritty's full grid/state model |
| Editor | **ropey** + **tree-sitter** + **async-lsp** + DAP client | ✅ keep, built | Buffer, highlighting, LSP, DAP, formatters implemented |
| Browser | **CEF** (Chromium) off-screen → wgpu texture, **chromiumoxide** (CDP) | 🟡 keep, isolate, ship last | CDP client built; CEF texture embed pending |
| Database | **ADBC** + **arrow-rs** + **sqlx** + embedded **DuckDB** | ✅ keep | SQLite driver built; Arrow columnar = scale story |
| Git | **git2** (libgit2) | ✅ keep, built | status/diff/stage/commit/push/branches/worktrees |
| Search | **ripgrep core (grep crates)** + tantivy (optional) | 🟢 add | File/content search; per-surface fast search built in-engine |
| IPC / ATP | UDS / named pipes; **JSON-RPC 2.0** + binary fast-path | ✅ keep, version it | Version the schema from v0 |
| Remote | **QUIC** (`quinn`) + **rustls** | ✅ keep | Encrypted, roaming, memory-safe TLS |
| Plugins | **WASM Component Model** (`wasmtime` + WIT) | ✅ keep | Sandboxed, years-horizon |
| Crypto / vault | **argon2** + AEAD (XChaCha20-Poly1305) + **zeroize** + **secrecy** | ✅ keep, built | enzo-vault implements envelope encryption |
| Biometric unlock | macOS **Keychain/LocalAuthentication**, Windows **Hello/DPAPI**, Linux **Secret Service** | 🟢 add | Second unlock path wrapping the Vault Key |
| Async | **tokio** | ✅ keep, hot-path rule | Keep render/input loop synchronous; tokio for I/O only |
| Config / script | **TOML** + **mlua** (or Rhai) | 🟡 KDL→TOML | Developers expect TOML |
| Packaging | single binary · `cargo-dist` · winget/Homebrew/AUR/AppImage | ✅ keep | |
| Supply chain | `cargo-deny`, `cargo-audit`, `cargo-vet`, SBOM, reproducible builds | ✅ keep | |

### 5.1 Reversibility map — where deliberation belongs
- **Irreversible — get right now:** Rust · daemon/client architecture · GPU
  abstraction + UI compositor · buffer/grid models · ATP schema · plugin ABI ·
  **workspace scoping model**.
- **Expensive but possible:** text-shaping stack · terminal grid lib · editor
  intelligence wiring · crypto primitives · reference-graph shape.
- **Cheap to change later (don't agonize):** config format · scripting language ·
  specific DB drivers · theme format · packaging · **the browser engine** ·
  search backend.

### Python?
**Rust for the engine, non-negotiable.** **Python is a first-class guest:** an
agent language over ATP, a `pip install enzo` automation SDK, and a future Jupyter-
kernel notebook surface. *Rust builds Enzo; Enzo loves Python users.*

---

## 6. Workspaces — the product's unit of work

A **workspace** is one loaded project and everything it needs. This is the
organizing idea of the whole product.

### 6.1 Project tabs
Projects appear as tabs on the menu bar. Add a project from the file explorer
("Open folder…"); it loads as a new tab. Switch projects like browser tabs.
Each tab is fully independent — its own surfaces, its own state, its own AI thread.

### 6.2 What a workspace contains
Every loaded project gets the identical, self-contained set:
- **IDE** — file tree + editor (tree-sitter highlight, LSP, DAP, formatter),
  rooted at the project directory; integrated git source-control view.
- **Terminal** — one or more PTY tabs, cwd defaulting to the project root.
- **Database** — connections for this project, each with its own **query tabs**
  (rename, history), a schema browser, and the table viewer/editor.
- **Browser** — one or more tabs with CDP devtools (Network / Console / Elements).

### 6.3 Per-project git worktree
Each workspace manages its own git worktree. Opening a project on a feature branch,
or spinning a worktree for a parallel branch, is a first-class workspace action —
not a terminal incantation. The git host (`git.*`, already built) backs status,
diffs, staging, commits, push/pull, branches, and `git.worktrees` /
`git.add_worktree`. The workspace owns which worktree path its surfaces point at.

### 6.4 Snapshots & resume (the sticky feature)
A workspace serializes to a snapshot: open files + cursors, terminal tabs +
scrollback handles, DB connections + query tabs, browser tabs + URLs, the active
AI thread, and the refs tray. Re-opening a project restores all of it. Because the
daemon owns the live state, snapshots are a serialization pass over existing
structures, and sessions can keep running while the renderer is closed.

### 6.5 Responsiveness at scale: lazy + hibernate
- **Lazy surface activation.** A surface's heavy resources (LSP process, DB
  connection, browser host) start only when the surface is first opened in that
  workspace — not on project load.
- **Project hibernation.** An idle workspace releases heavy resources (kills the
  browser host, parks LSP) while preserving serialized state; re-activating
  rehydrates on demand. Keeps "10 projects open" cheap.
- **Resource budgets.** Per-workspace caps so one runaway project can't starve the
  rest.

### 6.6 Simplicity contract
However loaded with features, the surface the user touches stays simple: a clean
project tab opens to a terminal; the other three surfaces and search/refs reveal
only when summoned. Density lives behind progressive disclosure, never in the face.

---

## 7. The four surfaces

Each surface exists *per workspace*. (Implementation status in §16.)

### 7.1 Terminal
`vte` parses the PTY byte stream off-thread; OSC 133/7/8 marks turn raw output into
addressable **blocks** (command + output + exit code + duration + cwd). Daemon owns
PTY + scrollback. Multiple PTY tabs per workspace. ATP blocks from the agent
composite in the same column; AI-CLI tool calls render as inline approval cards
(ACCEPT / REJECT / EDIT). Screenshot: `mockups/terminal.html`

### 7.2 IDE
`ropey` buffer + `tree-sitter` incremental highlighting + `async-lsp` client (rust-
analyzer, tsserver, pyright) + DAP client (CodeLLDB, debugpy) for breakpoints /
stepping / variables, plus formatter integration (rustfmt, black, prettier) and a
language registry mapping extensions → grammar + LSP + formatter. The editor widget
is the single biggest build. Screenshot: `mockups/ide.html`

### 7.3 Browser
CEF renders off-screen into a buffer uploaded as a wgpu texture (composited like
any panel). `chromiumoxide` speaks CDP → real Elements / Network / Console panels.
Multiple tabs per workspace. "Pick element → send to AI" pipes a CDP node-select
over ATP. **Sandboxed: separate process, no vault/secret access.**
Screenshot: `mockups/browser.html`

### 7.4 Database
ADBC / sqlx / embedded DuckDB; results return as Arrow record batches streamed into
a GPU-virtualized data grid (millions of rows at 120fps). **Multiple query tabs**
per connection (rename + per-tab history); a **schema browser** (tables, columns,
indexes); a **table viewer/editor** (view and edit cell values with injection-safe,
primary-key-anchored writes); **lazy pagination** for huge tables. SQL editor
reuses the IDE widget + a SQL language server. Harlequin's model with a GPU UI.
Screenshots: `mockups/database.html`, `mockups/db-connection.html`

---

## 8. Unified search & navigation

One keystroke (`⌘K`) opens **unified search** across the active workspace:
files, symbols (via LSP), terminal scrollback blocks, DB schema and recent
results, browser DOM/console lines, saved refs, and commands. Results are typed and
actionable: hitting Enter on a file opens it in the IDE, on a terminal block jumps
to it, on a DB table opens the viewer, on a command runs it.

Two complementary layers:
- **Global search (`⌘K`)** — fan-out across all surfaces of the current workspace
  (and optionally across all loaded projects). Backed by a daemon-side index over
  files (ripgrep-style content search), scrollback, schema, and captured DOM.
- **Scoped fast search (`⌘F`)** — surface-specific, instant, in-context:
  - **IDE** — in-buffer find/replace with regex, follows tree-sitter ranges.
  - **Terminal** — scrollback search with match highlighting and block jumps.
  - **DB query window** — search within the SQL editor and within result sets.
  - **Browser** — find-in-page + DevTools console/network filtering.

The **command palette** shares the `⌘K` surface (mode-switched), making every
action fuzzy-searchable and rebindable — discoverability without memorization.
Screenshots: `mockups/command-palette.html`, `mockups/universal-prompt.html`

ATP message family: `search.query`, `search.index`, `search.scoped`.

---

## 9. Cross-surface references — the reference graph

The keystone of the agent loop, and the feature competitors can't copy. Every
surface mints a typed **`Ref`** — never a text copy — carrying both a frozen
snapshot and a live pointer:

```
Ref {
  kind:     code | sql-result | table | dom-element | network-call
            | terminal-block | log-line | console-line
  source:   { workspace_id, panel_id, document }
  locator:  <live re-resolve handle>   // jump back; agent can re-read
  snapshot: <frozen capture>           // stable even if source changes
  render:   { chip, expanded }
}
```

- **Snapshot = stability** (agent reasons over exactly what you pointed at).
- **Locator = liveness** (click the chip to flash the source; `ref.resolve`
  re-reads current value; chip shows ● live / ◐ stale).

**One gesture everywhere:** select on any surface → `⌘E` → grabs to the refs tray
or the prompt. **Add anything as a reference anywhere:** a DB result row dropped
onto the terminal; a browser-rendered HTML element or a console line dropped into
the editor; a network call dropped into the AI composer. Bidirectional: refs drop
into the AI composer as typed context chips, OR into another panel (network call →
editor generates the fetch; table → struct; DOM → component). Code refs use
anchored ranges that follow edits; data/DOM/network refs are immutable snapshots
with `ref.subscribe` liveness.

Refs are **workspace-scoped** by default and can be promoted to cross-workspace.
ATP messages: `ref.create`, `ref.attach`, `ref.resolve`, `ref.reveal`,
`ref.subscribe`. **Secrets are scrubbed from all refs/blocks before reaching the
agent** (see security doc). Screenshot: `mockups/cross-link.html`

---

## 10. Agent Terminal Protocol (ATP)

Three layers, each a graceful fallback of the one above:

| Layer | Mechanism | Gives |
|---|---|---|
| 0 — Bytes | Raw VT/ANSI | Universal compatibility |
| 1 — Semantic shell | OSC 133 / 7 / 8 | Free blocks, exit codes, cwd, links from any shell |
| 2 — ATP | JSON-RPC 2.0 over `$ENZO_ATP_SOCK` | Native diffs, approvals, streaming, forms, references |

An agent detects `$ENZO_ATP_SOCK` (like `$TERM`) and speaks structured messages;
on any other terminal it falls back to ANSI. Zero lock-in.

**Message families.** Implemented today: `session.*`, `db.*`, `db.schema.*`,
`db.table.*`, `db.tabs.*`, `lsp.*`, `dap.*`, `browser.*`, `git.*`, `theme.*`,
`editor.*`, `prompt.*`, `block.*`, `display.*`. Designed / next: `workspace.*`,
`search.*`, `ref.*`, `stream.*`.

**The biggest product risk is ATP adoption.** Mitigation: Layers 0–1 make Enzo a
best-in-class normal terminal on day one (zero agent buy-in needed); we ship the
**Claude Code adapter ourselves** (`enzo-claude`, built); publish the spec openly.

---

## 11. UI engine: build vs. buy

| Layer | Verdict |
|---|---|
| GPU abstraction, windowing, text shaping, layout, a11y, 2D vector | **Buy** (wgpu, winit, cosmic-text, taffy, accesskit, vello) |
| Compositor, dock/split system, widget toolkit | **Build** — Enzo's identity |
| Effects pipeline (CRT / 8-bit shaders) | **Build** — the whole aesthetic |
| Editor widget, data grid, terminal grid | **Build** — bespoke surfaces |

**Recommendation:** prototype the compositor on GPUI to de-risk; then decide
whether to keep it or replace it with an own wgpu+vello+cosmic-text+taffy
compositor for full control of the CRT/pixel effects. Let the prototype, not the
ambition, make the call.

---

## 12. Themes

A theme is **pure data** (TOML), hot-reloadable, sandbox-safe, shareable via the
registry. Layered tokens: `palette → roles → syntax → fonts → effects`. The
**effects pipeline** (optional WGSL passes: scanlines, phosphor, curvature, bloom,
dither) applies to chrome/background only — **code text always stays crisp**,
effects off by default, force-disabled under reduced-motion / high-contrast.

Built-in library (implemented): **Enzo Dark** (default), **Enzo Light**,
**Tokyo Night**, **Matrix** (flagship 8-bit), **Game Boy DMG**, **Amber CRT**.
Roadmap 8-bit pack: NES, C64, PICO-8, ZX Spectrum, IBM CGA, Commodore PET, Apple
II. Roadmap modern: Catppuccin, Nord, Rosé Pine. Every theme ships high-contrast +
colorblind-safe variants (hard requirement). Themes are managed from the **settings
panel**. Screenshot: `mockups/theme-gallery.html`

---

## 13. UX — the surface the user lives in

**Governing idea: one universal prompt with visible intent.** The prompt defaults
to shell; a leading natural-language / `✧` flips to AI, `>` to commands, `@` to
references — and the **mode pill is always visible** so input never silently goes
somewhere unexpected. `⇥` cycles, `⎋` returns to shell.

Principles:
1. **One input, visible intent** — the pill is the source of truth, not a guess.
2. **The AI is ambient, not a destination** — summoned where the cursor is; replies
   render as inline blocks in the same column.
3. **One reference gesture everywhere** — `⌘E`, identical across all surfaces.
4. **One search everywhere** — `⌘K` unified, `⌘F` scoped; identical across surfaces.
5. **Progressive disclosure** — a clean terminal until you need more; IDE/DB/
   browser/refs/search appear on demand.
6. **Everything is undoable, including AI edits** — `⌘Z` spans editor + AI diffs.
7. **No modal dialogs** — approvals are inline blocks you can scroll past.
8. **Discoverable, not memorized** — hold `⌘` for which-key hints; `?` lists all;
   every action is fuzzy-searchable and rebindable.
9. **Switch projects, not context** — project tabs resume instantly and intact.

**Accessibility folded in:** semantic blocks let screen readers announce meaning;
reduced-motion kills effects; independent font scaling for chrome vs. code; full
keyboard operability. **Latency rule:** every interaction feels instant — sub-frame
echo, optimistic rendering, honest progress, never a spinner where a result could
stream. Screenshots: `mockups/universal-prompt.html`, `mockups/command-palette.html`,
`mockups/onboarding.html`

---

## 14. Authentication & session load

Everything sits behind a **load screen** shown before any workspace is rendered.

- **Master password is the root.** It derives (Argon2id) the Master Key, which
  unwraps a random Vault Key that AEAD-encrypts every secret. The master password
  *is* the private key to the workspace.
- **Touch ID / Windows Hello / Secret Service** are a *second, convenience* unlock
  path: the OS keystore wraps the same Vault Key. Biometrics never replace the
  master password; they unlock the same vault. Lose the device, the master
  password still recovers everything.
- **What unlock gates:** the credential vault (DB passwords, tokens), saved
  workspace snapshots, and any sync. Until unlocked, no secrets are in memory.
- **Load flow:** unlock → choose a project (recent list / open folder) → workspace
  rehydrates from its snapshot. New machine? Unlock pulls synced workspaces.

Secrets are decrypted only in memory (`secrecy` + `zeroize`, mlocked), never
logged, never in scrollback, **redacted from all agent context**. The browser (CEF)
runs sandboxed with no vault access. Screenshots: `mockups/unlock.html`,
`mockups/onboarding.html`

---

## 15. Security & credentials

See **`security-credentials.md`** for the full design. Summary:
- DB passwords stored via **envelope encryption**: master password → Argon2id →
  Master Key → unwraps a random Vault Key → AEAD-encrypts each secret
  (XChaCha20-Poly1305).
- The OS keystore (macOS Keychain / Windows Hello+DPAPI / Linux Secret Service) is
  a *second* convenience unlock path wrapping the same Vault Key, not the only one.
- Secrets decrypted only in memory (`secrecy` + `zeroize`, mlocked), never logged,
  never in scrollback, **redacted from all agent context**.
- The browser (CEF) runs sandboxed with no vault access.
- Refs and blocks are scrubbed of secrets before they can reach an agent.
Diagram: `diagrams/credential-encryption.svg`

---

## 16. Implementation status

**Built and tested (this codebase):**
- Daemon + ATP broker; PTY sessions; multi-tab terminal client.
- AI-CLI wrapper: `enzo-adapters` (`enzo-claude`), `prompt.*` / `block.*` /
  `display.*`, and the GPU **overlay** rendering approval cards with mouse +
  keyboard ACCEPT/REJECT/EDIT.
- `enzo-db`: SQLite driver, Arrow results, schema browser (`introspect`), table
  view/edit (`table`), pagination (`paginate`), query tabs + history (`tabs`).
- `enzo-editor`: ropey buffer, tree-sitter highlighting (Rust/Python/JS/JSON),
  language registry, LSP + DAP clients, formatter integration.
- `enzo-git`: status, diffs, stage/unstage, commit, branches, checkout, log,
  fetch/push, worktrees.
- `enzo-theme`: layered model, 6 built-in themes, hot-reload, `theme.*`.
- `enzo-vault` + `enzo-redact`: envelope-encrypted secrets, agent-context scrubbing.
- `enzo` orchestrator binary; CI; full test suite + clippy/fmt clean.

**Designed, not yet built (the four pillars + polish):**
- **Workspaces** (`workspace.*`): project tabs, per-project surface sets, snapshots,
  lazy/hibernate. (§6)
- **Unified + scoped search** (`search.*`). (§8)
- **Reference graph** (`ref.*`) — the keystone. (§9)
- **Auth/load screen** + biometric unlock UI. (§14)
- Per-project AI thread history + scoped memory; settings/workspace sync;
  CEF texture embed; DuckDB/Postgres drivers; collaboration; plugin/theme registry.

---

## 17. Build sequence

1. **v0.1 Terminal + ATP** — compositor, daemon, channel, block model, themes, the
   universal prompt, AI-CLI wrapper. ✅ landed.
2. **v0.2 Surfaces + workspace foundations** — editor (tree-sitter/LSP/DAP/format),
   database client (tabs/schema/table/pagination), git, browser CDP. ✅ surfaces
   landed; workspace scoping next.
3. **v0.3 The workspace product** — project tabs, per-project surface sets,
   snapshots/resume, lazy + hibernate, the auth/load screen.
4. **v0.4 The agent loop, complete** — reference graph (`ref.*`), unified search,
   per-project AI thread + memory.
5. **v0.5 Reach** — CEF browser embed, real-time collaboration, sync, plugin/theme
   registry, remote (QUIC).

Honest scope warning: each surface has a brutal long tail. Target is **"covers 90%
of the daily dev loop in one place with AI woven through,"** NOT feature parity with
Chrome DevTools / VS Code / DBeaver. That parity chase is a trap.

---

## 18. Open questions to revisit

- ATP standardization (push toward an open spec once proven).
- Web/remote client (WASM + WebGPU; the daemon/client split makes it natural).
- Real-time collaboration (multiple clients on one session already in the model).
- Local model integration for instant, offline command suggestion.
- Cross-workspace refs and search: default scope and privacy boundaries.
- Workspace sync conflict model (which machine wins when both edited offline).

---

## Index of artifacts

- `security-credentials.md` — credential encryption framework
- `diagrams/architecture.svg` — system architecture
- `diagrams/credential-encryption.svg` — key hierarchy + unlock flow
- `diagrams/reference-flow.svg` — cross-surface reference flow
- `mockups/onboarding.html` — first-run
- `mockups/unlock.html` — vault unlock (master password / biometrics)
- `mockups/terminal.html` — terminal surface
- `mockups/ide.html` — IDE surface
- `mockups/browser.html` — browser surface
- `mockups/database.html` — database surface
- `mockups/db-connection.html` — add connection + save to vault
- `mockups/cross-link.html` — side-by-side cross-surface references
- `mockups/theme-gallery.html` — theme picker
- `mockups/universal-prompt.html` — the universal prompt UX
- `mockups/command-palette.html` — command palette
- _(planned)_ `mockups/workspace-tabs.html` — project tabs + per-project surfaces
- _(planned)_ `mockups/unified-search.html` — ⌘K across surfaces
- _(planned)_ `mockups/settings.html` — settings & theme panel
