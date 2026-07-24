# Repository Guidelines

## Project Structure & Module Organization

`dtr` is a single Rust 2024 CLI crate. `src/main.rs` is the thin executable
entry point, while `src/lib.rs` coordinates parsing, planning, and execution.
Keep focused concerns in their existing modules: CLI definitions in
`src/cli.rs`, repository parsing in `src/repospec.rs`, operation planning in
`src/resolve.rs`, command execution in `src/command.rs`, configuration in
`src/config.rs`, and automatic installer detection in `src/install_detect.rs`.

Unit tests live beside the code they exercise. End-to-end Unix CLI tests and
their stub-command harness live in `tests/cli.rs`. Product documentation is in
`README.md`; design records and roadmap notes are under `devdocs/`. CI is
defined in `.github/workflows/ci.yml`.

## Build, Test, and Development Commands

- `cargo build --locked` builds the debug executable.
- `cargo run -- --help` runs the CLI from the checkout.
- `cargo install --path .` installs the current checkout locally.
- `cargo nextest run --locked` runs the complete test suite.
- `just check` runs the full local gate: rustfmt, Clippy with warnings denied,
  nextest, `cargo check`, actionlint, and `git diff --check`.

Run `just check` before committing. The gate requires `cargo-nextest`,
`actionlint`, and `just` in addition to the stable Rust toolchain.

## Coding Style & Naming Conventions

Use idiomatic Rust and let `cargo fmt` determine formatting. Use four-space
indentation, `snake_case` for functions/modules/tests, `PascalCase` for types,
and `SCREAMING_SNAKE_CASE` for constants. Keep `main.rs` minimal and prefer
typed plans over shell command strings. Preserve exact user-facing flags,
repospec rules, error wording, and credential-redaction boundaries.

## Testing Guidelines

Name tests after observable behavior, for example
`config_list_prints_configured_values_or_names`. Add focused unit tests for
parsing and validation, and integration tests when behavior crosses process,
filesystem, environment, or external-tool boundaries. Extend the existing
stub harness rather than invoking real forge or installer services. Use
`cargo nextest run <substring>` while iterating, then `just check`.

## Documentation, Commits & Pull Requests

Update `README.md` when CLI syntax, output, supported tools, configuration, or
behavior changes. Keep `devdocs/` plans consistent; historical superseded
statements use `※` rather than being silently rewritten.

Recent commits use concise imperative subjects such as `Add config key
discovery` and `Implement Rust repository installation`. Follow that pattern,
keep commits cohesive, and omit generated-tool attribution. Pull requests
should explain the user-visible change, note important design or security
tradeoffs, link relevant issues, and report `just check` results. Include CLI
output examples when command behavior changes; screenshots are generally not
needed for this terminal application.
