# Enzo — Design

An AI-native developer workspace: terminal + IDE + browser + database in one app,
unified by a single AI agent with structured context across all four surfaces.
8-bit aesthetic, modern engine. Open source, $0 to build and ship.

## Read in this order
1. [`design-document.md`](design-document.md) — the complete design (vision,
   architecture, stack, surfaces, ATP, references, themes, UX, build sequence).
2. [`security-credentials.md`](security-credentials.md) — the credential
   encryption framework (envelope encryption; master password as root key;
   Keychain / Windows Hello+DPAPI / Secret Service).

## Diagrams (`diagrams/`)
Open the `.svg` files in any browser.
- `architecture.svg` — daemon + client, dual-plane rendering.
- `credential-encryption.svg` — key hierarchy + unlock flow.
- `reference-flow.svg` — cross-surface reference flow.

## Screenshots (`mockups/`)
Open the `.html` files in any browser. One per page:

| Page | File |
|---|---|
| First run / onboarding | `mockups/onboarding.html` |
| Vault unlock | `mockups/unlock.html` |
| Terminal | `mockups/terminal.html` |
| IDE | `mockups/ide.html` |
| Browser | `mockups/browser.html` |
| Database | `mockups/database.html` |
| Add DB connection (vault) | `mockups/db-connection.html` |
| Cross-surface references | `mockups/cross-link.html` |
| Theme gallery | `mockups/theme-gallery.html` |
| Universal prompt (UX) | `mockups/universal-prompt.html` |
| Command palette | `mockups/command-palette.html` |

## Status
Draft 0.1 — ready to start. Build sequence: Terminal+ATP → Editor → Database →
Browser (sandboxed).

## Stack at a glance
Rust core · wgpu/vello/cosmic-text/taffy/accesskit UI · vte+portable-pty terminal ·
ropey+tree-sitter+LSP+DAP editor · CEF+CDP browser (sandboxed) · ADBC+Arrow+DuckDB
database · JSON-RPC ATP · QUIC+rustls remote · wasmtime plugins · Argon2id+
XChaCha20-Poly1305 vault. All MIT/Apache/BSD.
