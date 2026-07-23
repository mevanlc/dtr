default: check

# Run the complete local validation suite.
check:
    cargo fmt --all --check
    cargo clippy --all-targets --all-features --locked -- -D warnings
    cargo nextest run --locked
    cargo check --locked
    actionlint .github/workflows/ci.yml
    git diff --check
