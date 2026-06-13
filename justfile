# Enzo task runner. Install `just`: cargo install just
set shell := ["bash", "-uc"]

# Default: lint then test.
default: lint test

# Format all code.
fmt:
    cargo fmt --all

# Verify formatting without changing files.
fmt-check:
    cargo fmt --all --check

# Strict lints — warnings are errors.
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run the full test suite.
test:
    cargo test --all

# Coverage summary (excludes binary entry points).
cov:
    cargo llvm-cov --all --summary-only --ignore-filename-regex 'src/bin/'

# Coverage gate ≥90% (binary entry points excluded — only untestable OS setup lives there).
cov-gate:
    cargo llvm-cov --all --fail-under-lines 90 --ignore-filename-regex 'src/bin/'

# Supply-chain audit (licenses, advisories, sources).
deny:
    cargo deny check

# Everything CI runs.
ci: fmt-check lint test cov-gate
