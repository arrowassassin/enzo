# Enzo

An AI-native developer workspace — terminal + IDE + browser + database in one app,
unified by a single AI agent with structured context across every surface. 8-bit
aesthetic, modern engine. Open source, $0 to build and ship.

> Status: **early scaffold.** Design is complete; the first crate (the credential
> vault) is implemented and fully tested. See [`design/`](design/) for the full
> design and [`docs/SPIKE-compositor.md`](docs/SPIKE-compositor.md) for the next step.

## Layout

```
design/                  complete design document, security model, mockups, diagrams
docs/SPIKE-compositor.md  the compositor/latency spike that gates the v0.1 architecture
crates/
  enzo-vault/            envelope-encrypted credential vault (Argon2id + XChaCha20-Poly1305)
```

## Develop

Requires a recent stable Rust (edition 2024, Rust ≥ 1.96).

```bash
just            # lint + test
just ci         # fmt-check + clippy + test + 100% line-coverage gate
just cov        # coverage summary
```

Or directly:

```bash
cargo test --all
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all
```

### Quality gates
- **rustfmt** — enforced (`rustfmt.toml`, edition 2024).
- **clippy** — `pedantic`, warnings-as-errors.
- **tests** — 100% of reachable lines covered (the only uncovered regions are the
  panic arms of documented infallible invariants).
- **pre-commit** — runs fmt + clippy + tests. Enable once per clone:
  ```bash
  git config core.hooksPath .githooks
  ```
- **supply chain** — `cargo deny` (permissive licenses only, advisory + source checks).

## License

Licensed under MIT.
