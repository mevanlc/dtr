# dtr first MVP roadmap

Status: in progress — 4 of 5 phases complete (2026-07-23)

## Document hierarchy

The names have two levels:

- **First MVP** is the product milestone and finish line defined by this file.
- **MVP00** through **MVP04** are implementation phases within that milestone.
- `PLAN-MVPnn.md` is the detailed functional requirements and validation record
  for one phase.

The phase count is deliberate. New work should not silently become another
`MVPnn` phase: changing the first-MVP finish line or phase count requires an
explicit edit here.

## First-MVP finish line

The first MVP fulfills dtr's original repo-oriented promise on macOS and Linux:

- One deterministic `<dtr-repospec>` grammar for local paths, forge shorthand,
  forge URLs, generic Git URLs, and SCP-like Git remotes.
- `dtr clone` with GitHub/GitLab awareness and native Git clone options.
- `dtr install` from repositories using the Go, Rust/Cargo, uv, pipx, and npm
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
| [MVP03](PLAN-MVP03.md) | Complete | Python repository installation through explicit `--uv` and `--pipx` backends. |
| MVP04 | Planned | npm repository installation plus cross-backend consistency, documentation, and first-MVP release closure. |

This is 4 of 5 phases complete. That fraction describes roadmap position, not
effort: phases are intentionally scoped around coherent, independently
validated product increments rather than equal amounts of work.

## Phase intent

### MVP03 — Python repository installation (complete)

[PLAN-MVP03.md](PLAN-MVP03.md) defines and validates:

- `dtr install --uv <dtr-repospec>`.
- `dtr install --pipx <dtr-repospec>`.
- Local and remote repository mapping without wrapping PyPI package specs.
- An explicit native-argument boundary where the underlying tools require one.
- GitHub auto-switch behavior wherever the selected backend fetches Git itself.
- PATH-isolated tests, live local-repository smoke validation, and macOS/Linux
  gates.

Automatic Python backend selection and repository inspection remain parked.

### MVP04 — npm and first-MVP closure

Before implementation, `PLAN-MVP04.md` will settle npm's local and Git source
mapping. The phase then completes:

- `dtr install --npm <dtr-repospec>` without wrapping npm registry specs.
- Consistent selector, argument-forwarding, auth, explain, and error behavior
  across all first-MVP install backends.
- End-to-end README and help examples for every supported operation.
- Full macOS/Linux validation and an explicit first-MVP release-readiness audit.

MVP04 may repair inconsistencies found in completed phases, but it is not a
catch-all for unrelated features.

## Explicitly after the first MVP

- Automatic installer detection by inspecting a repository.
- Literal `scp://` and `sftp://` download/staging workflows.
- Configurable default forge, host, source, or account for shorthand repospecs.
- GitHub Enterprise, GitLab, and generic-host account selection.
- Persistent checkout-to-account binding for later fetch and push operations.
- Windows support.
- JSON output, plugins, `doctor`, update, or uninstall orchestration.

These may inform current architecture, but none blocks the first-MVP finish
line.
