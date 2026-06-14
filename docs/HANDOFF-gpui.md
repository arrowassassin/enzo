# Enzo GPUI client — continuation handoff

Status as of the GPUI migration. The client UI was rebuilt from egui to **GPUI**
(Zed's GPU framework); the Rust **backends and the ATP daemon are unchanged** —
only the view layer was rewritten. Everything lives in the isolated crate
`crates/enzo-gpui` (its own `[workspace]` so its heavy GPUI deps don't bloat the
main build).

## Task status

| Task | State |
|---|---|
| **1 — Database** | ✅ complete — real connect/seed/schema/browse/clickable-pager/query/errors, editable SQL, add-connection dialog, PK-anchored cell editing. No demo data. |
| **3 — Terminal** | ✅ complete — real PTY, ANSI-colour grid, full keyboard (Enter/Ctrl/Alt), **OSC-133 semantic command blocks** (daemon shell-integration → VT capture → block cards), alt-screen→grid fallback. |
| **4 — IDE** | ✅ file tree + **gpui-component `CodeEditor`** (ropey + tree-sitter for ~35 langs + LSP hooks + folding) + Enzo theme (dark, palette-mapped, dark syntax theme) + ⌘S save. **Remaining:** Enzo-tuned syntax token colours, `editor.format` action, real LSP server (rust-analyzer/pyright), debugger panel. |
| **2 — Themes** | ⏳ pending — live theme switching. Now also means driving our palette through gpui-component's `Theme` so the whole app (incl. editor) switches together (`apply_enzo_theme` in `main.rs` is the seam). |
| **5 — Workspaces / Search / Refs / Auth** | ⏳ pending. `design/mockups/workspaces.html` exists for §6. |
| **v0.2** | ⏳ pending (`design/design-document.md`). |

**Tests:** daemon **27 lib + 27 integration**, client **43** — all green.

## Architecture / key files (`crates/enzo-gpui/src/`)

- `main.rs` — `EnzoApp` root view, dock/surface dispatch, ATP drain loop
  (`cx.spawn` + 30ms timer), per-surface key-context scoping, `apply_enzo_theme`.
- `atp.rs` — background tokio thread owning the JSON-RPC/UDS connection;
  `Command` out / `Incoming` in over `std::mpsc`; connect-retry.
- `database.rs` — `DbState` + state-driven surface (sidebar/tab-bar/grid/dialog).
- `terminal.rs` — terminal sidebar/tab-bar/status-bar; `terminal_state.rs` is the
  `vte` VT100 state machine + OSC-133 block model (unit-tested).
- `ide.rs` — file tree + gpui-component editor wiring.
- `text_input.rs` — reusable single-line input (ported from gpui's example) for
  SQL + dialog fields.
- `theme.rs` — exact-mockup palette, fonts, Tabler icon glyphs.

**Daemon changes** (`crates/enzo-daemon/src/`): `shell_integration.rs` (OSC-133
bash rc injected at spawn), `session.rs` (kills child on drop), `state.rs` +
`atp/mod.rs` (idempotent `session.spawn`).

## Build & run (portable — pinned git deps, no local forks needed)

```bash
# deps: Rust stable + C compiler; Linux also: libfontconfig-dev libvulkan1
#   libwayland-dev libx11-xcb-dev libxkbcommon-x11-dev libzstd-dev cmake pkg-config
cd <repo> && cargo run -p enzo-daemon &        # ATP socket: /tmp/enzo-atp.sock
cd crates/enzo-gpui && cargo run               # first build ~15-20 min
```

gpui is pinned to `df9c9f0` (zed) and gpui-component to `cda0fc7` via
`Cargo.lock`; both are git deps with no `[patch]`.

## Verification in headless cloud (no display)

GPUI renders via Vulkan/lavapipe; **screenshots aren't possible** (the software
present path is unreadable by X capture tools, and GPUI has no Linux offscreen
renderer). Verify instead via: `cargo build`/`cargo test`, a **review sub-agent**
that diffs code against the mockups, and **end-to-end** runs under `Xvfb` with
**`xdotool` keystroke injection** + checking the daemon log (this proved the
terminal runs real commands and the IDE requests highlights). Visual QA happens
on a machine with a display.

## Opportunity

Adopting gpui-component also unlocks its **`Table`** (could power the
million-row DB grid), **`Tree`**, dropdowns, etc. — progressively replace
hand-rolled pieces with its battle-tested components where it helps.

## Next

Finish Task 4 polish (Enzo syntax colours + `editor.format`) → Task 2 (live
theme switching, palette-through-gpui-component) → Task 5 → v0.2.
