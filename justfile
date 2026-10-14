default: check

# Run the complete local validation suite.
check:
    cargo fmt --all --check
    cargo clippy --all-targets --all-features --locked -- -D warnings
    cargo nextest run --locked
    cargo test --doc --locked
    cargo check --locked
    uv run scripts/test_release.py
    sh scripts/test-cargo-target-cache.sh
    actionlint
    git diff --check
