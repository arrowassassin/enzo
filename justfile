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

# Coverage summary.
cov:
    cargo llvm-cov --all --summary-only

# Coverage gate: fail if any reachable line is untested.
cov-gate:
    cargo llvm-cov --all --fail-under-lines 100

# Supply-chain audit (licenses, advisories, sources).
deny:
    cargo deny check

# Everything CI runs.
ci: fmt-check lint test cov-gate
