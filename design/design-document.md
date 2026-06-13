# Enzo — Design Document

> An AI-native developer workspace with a terminal at its heart.
> 8-bit aesthetic, modern engine. Open source, $0 to build and ship.

Version: 0.1 (draft) · Last updated: 2026-06-13

---

## 1. Vision

Enzo is not "another terminal emulator." It is a GPU application platform whose
home surface is a terminal, and which folds the four tools a developer lives in —
**terminal, code editor (IDE), web browser, and database client** — into one app,
unified by a single thing nobody else has: **one AI agent with simultaneous,
structured context across all four surfaces.**

The reframing that makes everything else possible: the moment Enzo gained a GPU
*semantic plane* (rich UI beyond the 80×24 character grid), it stopped being a
terminal and became an IDE-class application that *contains* a terminal. A browser
panel, an editor panel, and a DB panel are then just more panels on the same
compositor.

### The one reason to unify
"VS Code + Chrome + DBeaver + iTerm" already exists and is free. The only
justification for building Enzo is the **cross-surface agent loop**: the agent can
read the failing `GET /api/me` 401 in the browser's network tab, jump to the auth
handler in the editor, query the `users` table to check the row, and propose the
fix — in one instruction, with all three as structured context. No combination of
separate tools can hand an AI that unified view. If that loop isn't magic, Enzo
shouldn't be built.

---

## 2. Non-negotiable principles

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

---

## 3. Architecture

### 3.1 Daemon + client split
The single most important structural decision. A headless **session daemon** owns
all state (PTYs, scrollback, DB connections, LSP/DAP processes, the credential
vault, ATP connections). A disposable **GPU UI client** renders it.

This one move buys: crash resilience (renderer panic loses nothing), persistent &
detachable sessions, mosh-style network roaming, multiple windows over one
session, and headless/CI use.

```
+-----------------------------------------------------------+
|                     ENZO UI CLIENT                        |
|  GPU window (wgpu: Metal / DX12 / Vulkan / WebGPU)        |
|                                                           |
|  +----------------+      +---------------------------+    |
|  |  COMPAT PLANE  |      |     SEMANTIC PLANE        |    |
|  |  xterm grid    |      | blocks, diffs, widgets,   |    |
|  |  (raw VT)      |      | editor, browser, db, agent|    |
|  +--------+-------+      +-------------+-------------+    |
|           +---- GPU compositor --------+                  |
|  AccessKit a11y tree -> UIA / AT-SPI / NSAccessibility    |
+---------------------------+-------------------------------+
                            |  local IPC (UDS / named pipe)
                            |  or QUIC when remote
+---------------------------+-------------------------------+
|                  ENZO SESSION DAEMON                      |
|  PTY mgr (ConPTY / openpty)                               |
|  Scrollback store (chunked, compressed, mmap)            |
|  Block model + OSC 133/7/8 parser                        |
|  ATP broker (Agent Terminal Protocol, JSON-RPC)          |
|  Reference store (cross-surface refs)                    |
|  Credential vault (envelope-encrypted)                   |
|  LSP/DAP host . DB connections (ADBC) . WASM plugins     |
|  CEF browser host (SANDBOXED, no vault access)           |
+----+--------------------+--------------------+-----------+
     |                    |                    |
  your shell          Claude Code          DB engines /
  zsh/fish/pwsh       (or any agent,        Postgres /
                       speaks ATP)          DuckDB / ...
```

### 3.2 Dual-plane rendering
- **Compatibility plane** — a bulletproof xterm/VT emulator. Runs any app.
- **Semantic plane** — GPU-composited rich UI: blocks, diffs, the editor, the
  browser texture, the data grid, agent UI. A legacy full-screen app simply takes
  over a block in "raw" mode.

---

## 4. Technology stack (all MIT / Apache-2.0 / BSD — $0)

Revalidated 2026-06-13. Each row carries a verdict; see §4.1 for the reversibility
map and the rationale behind the adjusted choices.

| Layer | Technology | Verdict | Notes |
|---|---|---|---|
| Core language | **Rust** | ✅ keep | Memory-safe, single binary, the ecosystem already exists here |
| GPU | **wgpu** (WGSL shaders) | ⚠️ keep, benchmark | One abstraction over Metal/DX12/Vulkan/WebGPU + future WebGPU client. Spike keystroke latency before locking; keep behind a thin renderer trait (fallback: `blade`/native) |
| UI compositor | **Own**, on wgpu + cosmic-text + taffy | 🔴 build own | Prototype on **GPUI** to learn, but do NOT make it load-bearing — coupling to Zed's roadmap + weaker Win/Linux maturity is the biggest irreversible risk |
| 2D vector / effects | hand-written **WGSL** passes; **vello** optional | 🟡 demote vello | 8-bit/CRT effects don't need a full vector engine; vello is pre-1.0 and GPU-heavy. Adopt only if complex vector UI appears |
| Text shaping | **cosmic-text** + **swash** (+ rustybuzz) | ✅ keep | rustybuzz (HarfBuzz port) underneath is battle-tested. Never hand-roll |
| Layout | **taffy** | ✅ keep | Flexbox math |
| Windowing/input | **winit** | ✅ keep | |
| Accessibility | **accesskit** | ✅ keep | UIA / AT-SPI / NSAccessibility — high value |
| Terminal core | **vte** + **alacritty_terminal** + **portable-pty** | ✅ keep, +grid | Use Alacritty's full grid/state model, not just the parser |
| Editor | **ropey** + **tree-sitter** + **async-lsp** + DAP client | ✅ keep | Industry-standard; future-proof |
| Browser | **CEF** (Chromium) off-screen → wgpu texture, **chromiumoxide** (CDP) | 🟡 keep, isolate, ship last | Heaviest + CVE magnet, but only path to consistent Chromium fidelity + DevTools. Soft lock-in (isolated surface) |
| Database | **ADBC** + **arrow-rs** + **sqlx** + embedded **DuckDB** | ✅ keep | Arrow columnar = the millions-of-rows scalability story |
| IPC / ATP | UDS / named pipes; **JSON-RPC 2.0** + binary fast-path | ✅ keep, version it | Version the schema from v0 — it's as locked-in as the code |
| Remote | **QUIC** (`quinn`) + **rustls** | ✅ keep | Encrypted, roaming, memory-safe TLS |
| Plugins | **WASM Component Model** (`wasmtime` + WIT) | ✅ keep | Sandboxed, years-horizon; WIT tooling young but strategically right |
| Crypto | **argon2** (RustCrypto) + AEAD via **ring**/**dryoc** + **zeroize** + **secrecy** | 🟡 audited AEAD | Prefer an audited cipher impl over RustCrypto's AEAD; Argon2id fine |
| Async | **tokio** | ✅ keep, hot-path rule | Keep render/input loop synchronous; tokio for I/O only |
| Config / script | **TOML** + **mlua** (or Rhai) | 🟡 KDL→TOML | Developers expect TOML; soft lock-in regardless |
| Packaging | single binary · `cargo-dist` · winget/Homebrew/AUR/AppImage | ✅ keep | |
| Supply chain | `cargo-deny`, `cargo-audit`, `cargo-vet`, SBOM, reproducible builds | ✅ keep | |

### 4.1 Reversibility map — where deliberation belongs

The fear is "hard to switch later." Most of the stack is swappable behind
interfaces; only a few choices are truly irreversible. Spend the care here:

- **Irreversible — get right now:** Rust · daemon/client architecture · GPU
  abstraction + UI compositor · buffer/grid models · ATP schema · plugin ABI.
- **Expensive but possible:** text-shaping stack · terminal grid lib · editor
  intelligence wiring · crypto primitives.
- **Cheap to change later (don't agonize):** config format · scripting language ·
  specific DB drivers · theme format · packaging · **the browser engine** (isolated
  surface — CEF→Servo later won't touch the core).

### 4.2 Deltas from the revalidation pass
1. Own the compositor; prototype on GPUI but don't depend on it long-term.
2. Demote vello to optional; do effects as hand-written WGSL passes.
3. Add `alacritty_terminal` for the grid model (not just `vte`).
4. Swap the AEAD to an audited impl (`ring`/`dryoc`); keep Argon2id.
5. Config KDL → TOML (KDL optional).
6. Version the ATP schema from v0; benchmark wgpu latency on a spike first.

### Python?
**Rust for the engine, non-negotiable** (the GIL, GC pauses, and lack of single-
binary distribution rule Python out of the hot path; Textual/Harlequin are TUIs
that ride inside a terminal, not GPU apps). **Python is a first-class guest:** an
agent language over ATP, a `pip install enzo` automation SDK, and a future Jupyter-
kernel notebook surface. The line: *Rust builds Enzo; Enzo loves Python users.*

---

## 5. The four surfaces

### 5.1 Terminal
`vte` parses the PTY byte stream off-thread; OSC 133/7/8 marks turn raw output into
addressable **blocks** (command + output + exit code + duration + cwd). Daemon owns
PTY + scrollback. ATP blocks from the agent composite in the same column.
Screenshot: `mockups/terminal.html`

### 5.2 IDE
`ropey` buffer + `tree-sitter` incremental highlighting + `async-lsp` client (rust-
analyzer, tsserver, pyright) + DAP client (CodeLLDB, debugpy) for breakpoints /
stepping / variables. The editor widget is the single biggest build.
Screenshot: `mockups/ide.html`

### 5.3 Browser
CEF renders off-screen into a buffer uploaded as a wgpu texture (composited like
any panel). `chromiumoxide` speaks CDP → real Elements / Network / Console panels.
"Pick element → send to AI" pipes a CDP node-select over ATP.
**Sandboxed: separate process, no vault/secret access.**
Screenshot: `mockups/browser.html`

### 5.4 Database
ADBC / sqlx / embedded DuckDB; results return as Arrow record batches streamed into
a GPU-virtualized data grid (millions of rows at 120fps). SQL editor reuses the IDE
widget + a SQL language server. Harlequin's model with a GPU UI.
Screenshots: `mockups/database.html`, `mockups/db-connection.html`

---

## 6. UI engine: build vs. buy

| Layer | Verdict |
|---|---|
| GPU abstraction, windowing, text shaping, layout, a11y, 2D vector | **Buy** (wgpu, winit, cosmic-text, taffy, accesskit, vello) — reinventing these is malpractice |
| Compositor, dock/split system, widget toolkit | **Build** — Enzo's identity |
| Effects pipeline (CRT / 8-bit shaders) | **Build** — the whole aesthetic |
| Editor widget, data grid, terminal grid | **Build** — bespoke surfaces |

**Recommendation:** prototype the compositor on **GPUI** (Zed's framework, Apache-
2.0) to de-risk and save 6–12 months; then decide whether to keep it or replace it
with an own wgpu+vello+cosmic-text+taffy compositor for full control of the CRT/
pixel effects. Let the prototype, not the ambition, make the call.

---

## 7. Agent Terminal Protocol (ATP)

Three layers, each a graceful fallback of the one above:

| Layer | Mechanism | Gives |
|---|---|---|
| 0 — Bytes | Raw VT/ANSI | Universal compatibility |
| 1 — Semantic shell | OSC 133 / 7 / 8 | Free blocks, exit codes, cwd, links from any shell |
| 2 — ATP | JSON-RPC 2.0 over `$ENZO_ATP_SOCK` | Native diffs, approvals, streaming, forms, references |

An agent detects `$ENZO_ATP_SOCK` (like `$TERM`) and speaks structured messages;
on any other terminal it falls back to ANSI. Zero lock-in. Message families:
`block.*`, `prompt.*`, `stream.*`, `query.*`, `register.*`, `ref.*`.

**The biggest product risk is ATP adoption.** Mitigation: Layers 0–1 make Enzo a
best-in-class normal terminal on day one (zero agent buy-in needed); ship the
Claude Code adapter ourselves; publish the spec openly.

---

## 8. Cross-surface references

The keystone of the agent loop. Every surface mints a typed **`Ref`** — never a
text copy — carrying both a frozen snapshot and a live pointer:

```
Ref {
  kind:     code | sql-result | table | dom-element | network-call
            | terminal-block | log-line
  source:   { panel_id, document }
  locator:  <live re-resolve handle>   // jump back; agent can re-read
  snapshot: <frozen capture>           // stable even if source changes
  render:   { chip, expanded }
}
```

- **Snapshot = stability** (agent reasons over exactly what you pointed at).
- **Locator = liveness** (click the chip to flash the source; `ref.resolve`
  re-reads current value; chip shows ● live / ◐ stale).

**One gesture everywhere:** select on any surface → `⌘E` → grabs to the tray or the
prompt. Bidirectional: refs drop into the AI composer as typed context chips, OR
into another panel (network call → editor generates the fetch; table → struct;
DOM → component). Code refs use anchored ranges that follow edits; data/DOM/network
refs are immutable snapshots with `ref.subscribe` liveness.

ATP messages: `ref.create`, `ref.attach`, `ref.resolve`, `ref.reveal`,
`ref.subscribe`. **Secrets are scrubbed from all refs/blocks before reaching the
agent** (see security doc).
Screenshot: `mockups/cross-link.html`

---

## 9. Themes

A theme is **pure data** (KDL/TOML), hot-reloadable, sandbox-safe, shareable via the
registry — so the community ships hundreds without code review, and a theme can
never read scrollback. Layered tokens: `palette → roles → syntax → fonts →
effects`. The **effects pipeline** (optional WGSL passes: scanlines, phosphor,
curvature, bloom, dither) applies to chrome/background only — **code text always
stays crisp**, effects off by default, force-disabled under reduced-motion / high-
contrast.

**8-bit pack (headliners):** Matrix (flagship), Game Boy DMG, NES, C64, PICO-8,
Amber CRT, ZX Spectrum, IBM CGA, Commodore PET, Apple II — each on its console's
real hardware palette. **Modern pack:** Enzo Dark (default), Tokyo Night,
Catppuccin, Nord, Rosé Pine, a light theme. Every theme ships high-contrast +
colorblind-safe variants (hard requirement).
Screenshot: `mockups/theme-gallery.html`

---

## 10. UX — the surface the user lives in

**Governing idea: one universal prompt with visible intent.** The prompt defaults
to shell; a leading natural-language / `✧` flips to AI, `>` to commands, `@` to
references — and the **mode pill is always visible** so input never silently goes
somewhere unexpected. `⇥` cycles, `⎋` returns to shell.

Seven principles:
1. **One input, visible intent** — the pill is the source of truth, not a guess.
2. **The AI is ambient, not a destination** — summoned where the cursor is; replies
   render as inline blocks in the same column.
3. **One reference gesture everywhere** — `⌘E`, identical across all surfaces.
4. **Progressive disclosure** — it's a clean terminal until you need more; IDE/DB/
   browser appear on demand.
5. **Everything is undoable, including AI edits** — `⌘Z` spans editor + AI diffs.
6. **No modal dialogs** — approvals are inline blocks you can scroll past; nothing
   steals focus.
7. **Discoverable, not memorized** — hold `⌘` for which-key hints; `?` lists all;
   every action is fuzzy-searchable and rebindable.

**Accessibility folded in:** semantic blocks let screen readers announce meaning
("command succeeded, 2.1s"); reduced-motion kills effects; independent font scaling
for chrome vs. code; full keyboard operability.

**Latency rule:** every interaction feels instant — sub-frame echo, optimistic
rendering, honest progress, never a spinner where a result could stream.
Screenshots: `mockups/universal-prompt.html`, `mockups/command-palette.html`,
`mockups/onboarding.html`

---

## 11. Security & credentials

See **`security-credentials.md`** for the full design. Summary:
- DB passwords stored via **envelope encryption**: master password → Argon2id →
  Master Key → unwraps a random Vault Key → AEAD-encrypts each secret
  (XChaCha20-Poly1305).
- **The master password is the root private key** ("Enzo's password acts as the
  private key"). The OS keystore (macOS Keychain / Windows Hello+DPAPI / Linux
  Secret Service) is a *second* convenience unlock path wrapping the same Vault Key,
  not the only one.
- Secrets decrypted only in memory (`secrecy` + `zeroize`, mlocked), never logged,
  never in scrollback, **redacted from all agent context**.
- The browser (CEF) runs sandboxed with no vault access.
Diagram: `diagrams/credential-encryption.svg`
Screenshots: `mockups/unlock.html`, `mockups/db-connection.html`

---

## 12. Build sequence

1. **v0.1 Terminal + ATP** — establishes compositor, daemon, channel, block model,
   themes, the universal prompt. Best-in-class terminal standalone.
2. **v0.2 Editor** — biggest single widget; reused everywhere after. Tree-sitter +
   LSP + DAP.
3. **v0.3 Database** — cheap: reuses editor + data grid; adds ADBC + vault +
   credential UX.
4. **v0.4 Browser** — heaviest embed, lowest novel code (CDP does the work); ships
   sandboxed and optional.

Honest scope warning: each surface has a brutal long tail. Target is **"covers 90%
of the daily dev loop in one place with AI woven through,"** NOT feature parity with
Chrome DevTools / VS Code / DBeaver. That parity chase is a trap.

---

## 13. Open questions to revisit

- ATP standardization (push toward an open spec once proven).
- Web/remote client (WASM + WebGPU; the daemon/client split makes it natural).
- Real-time collaboration (multiple clients on one session already in the model).
- Local model integration for instant, offline command suggestion.

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
