# dtr first MVP roadmap

Status: complete — 5 of 5 phases complete; automatic install selection was
delivered as a post-MVP increment (2026-07-23)

`※` marks a historical statement or section whose interface or roadmap status
has since been superseded by [PLAN-AUTO-INSTALL.md](PLAN-AUTO-INSTALL.md).

## Document hierarchy

The names have two levels:

- **First MVP** is the product milestone and finish line defined by this file.
- **MVP00** through **MVP04** are implementation phases within that milestone.
- `PLAN-MVPnn.md` is the detailed functional requirements and validation record
  for one phase.
- [PLAN-AUTO-INSTALL.md](PLAN-AUTO-INSTALL.md) records the completed post-MVP
  automatic install-selection increment.

The phase count is deliberate. New work should not silently become another
`MVPnn` phase: changing the first-MVP finish line or phase count requires an
explicit edit here.

## First-MVP finish line

The first MVP fulfills dtr's original repo-oriented promise on macOS and Linux:

- One deterministic `<dtr-repospec>` grammar for local paths, forge shorthand,
  forge URLs, generic Git URLs, and SCP-like Git remotes.
- `dtr clone` with GitHub/GitLab awareness and native Git clone options.
- ※ `dtr install` from repositories using the Go, Rust/Cargo, uv, pipx, and npm
  ecosystems explicitly selected by the user.
- Process-scoped GitHub account auto-switching for operations where an explicit
  repository owner identifies an allowlisted GitHub CLI account.
- Exact, secret-free `--explain` output driven by the same command plan as real
  execution.
- Direct argv execution without shell command construction.
- Focused tests and validation on both supported operating systems.

Registry package installation is outside the finish line. The native tools
already provide that surface well; dtr exists to smooth out their inconsistent
local-filesystem, Git-remote, and forge-aware repository surfaces.

## Phases

| Phase | Status | Product increment |
|---|---|---|
| [MVP00](PLAN-MVP00.md) | Complete | Common repospec resolver, clone, Go repository install, explain, and macOS/Linux foundation. |
| [MVP01](PLAN-MVP01.md) | Complete | Configuration and race-free, process-scoped GitHub account auto-switching. |
| [MVP02](PLAN-MVP02.md) | Complete | Rust/Cargo local and Git repository installation, including GitHub auth propagation. |
| ※ [MVP03](PLAN-MVP03.md) | Complete | Python repository installation through explicit `--uv` and `--pipx` backends. |
| [MVP04](PLAN-MVP04.md) | Complete | npm repository installation plus cross-backend consistency, documentation, and first-MVP release closure. |

This is 5 of 5 phases complete. That fraction describes roadmap position, not
effort: phases are intentionally scoped around coherent, independently
validated product increments rather than equal amounts of work.

## Phase intent

### ※ MVP03 — Python repository installation (complete)

[PLAN-MVP03.md](PLAN-MVP03.md) defines and validates:

- `dtr install --uv <dtr-repospec>`.
- `dtr install --pipx <dtr-repospec>`.
- Local and remote repository mapping without wrapping PyPI package specs.
- An explicit native-argument boundary where the underlying tools require one.
- GitHub auto-switch behavior wherever the selected backend fetches Git itself.
- PATH-isolated tests, live local-repository smoke validation, and macOS/Linux
  gates.

At first-MVP closure, automatic Python backend selection and repository
inspection remained parked. They were subsequently delivered by
[the automatic install-selection increment](PLAN-AUTO-INSTALL.md).

### ※ MVP04 — npm and first-MVP closure

[PLAN-MVP04.md](PLAN-MVP04.md) defines and validates:

- `dtr install --npm <dtr-repospec>` without wrapping npm registry specs.
- Consistent selector, argument-forwarding, auth, explain, and error behavior
  across all first-MVP install backends.
- End-to-end README and help examples for every supported operation.
- Full macOS/Linux validation and an explicit first-MVP release-readiness audit.

MVP04 may repair inconsistencies found in completed phases, but it is not a
catch-all for unrelated features.

## ※ First-MVP completion audit (historical)

Completed on 2026-07-23 against the finish line above. Counts and interface
spellings in this section are the first-MVP closure snapshot; current behavior
and validation are recorded in [PLAN-AUTO-INSTALL.md](PLAN-AUTO-INSTALL.md).

| Finish-line requirement | Completion evidence |
|---|---|
| One deterministic repospec grammar | `RepoSpec` classifies every documented local, forge, generic URL, SCP-like, and bare-name family before backend planning. Unit and PATH-isolated integration tests cover those mappings. |
| Forge-aware clone | `dtr clone` selects gh, glab, or Git fallback; preserves native Git options; and implements `-O` / `-D`. MVP00 and current regression tests cover exact plans. |
| Five explicit repository installers | Install help exposes Go, Rust/Cargo, uv, pipx, and npm. The 107-test suite covers exact local and remote argv for every backend; real local packages have been installed through every non-Go installer. |
| Process-scoped GitHub account selection | Clone and Git-fetching installers select only allowlisted explicit owners. Tests cover matched, unmatched, bare, inherited-config, and fail-closed behavior; live explains selected both configured accounts without changing the active account. |
| Exact, secret-free explain | Explain and execution consume the same typed `CommandPlan`. Exact-output tests verify every operation family; token and derived header values remain secret environment only. |
| Direct argv execution | All final operations run with `std::process::Command` and an argv vector. No operation constructs or invokes a shell command string. |
| macOS and Linux support | macOS passes fmt, locked clippy, 107 nextest tests, locked check, actionlint, and diff validation. Linux Rust 1.97.1 passes fmt, locked clippy, all 107 tests plus doctests, and locked check. |

The first-MVP feature set is release-ready as dtr 0.1.0 and installed from this
checkout. Publishing, pushing, and creating a Git tag are separate user-directed
release actions and were not performed by this roadmap closure.

## Delivered after the first MVP

- [Automatic install tool selection](PLAN-AUTO-INSTALL.md), including the
  unified `--tool` option, conservative root-manifest inference, Python backend
  selection, forge API inspection, and filtered Git fallback.
- `gcl` clone-compatibility follow-up: repository-root fragment stripping,
  `-U` / `--upstream-remote-name`, and forge-aware GitHub SSH references.
- GitHub browser URL normalization: well-known paths after `owner/repo` and
  HTTP(S) query parameters are stripped before clone or installation planning.

## Remaining after the first MVP

- Literal `scp://` and `sftp://` download/staging workflows.
- Configurable default forge, host, source, or account for shorthand repospecs.
- GitHub Enterprise, GitLab, and generic-host account selection.
- Persistent checkout-to-account binding for later fetch and push operations.
- Windows support.
- JSON output, plugins, `doctor`, update, or uninstall orchestration.

These may inform current architecture, but none blocks the first-MVP finish
line.
