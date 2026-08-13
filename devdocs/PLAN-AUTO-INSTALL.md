# dtr automatic install tool selection plan

Status: implemented and validated on macOS (2026-07-23; kit follow-up 2026-08-11)

Roadmap: completed post-[first MVP](FIRST-MVP.md) increment

This document records the current install interface and supersedes the explicit
selector spellings in the historical `PLAN-MVP00.md` through `PLAN-MVP04.md`
phase records. It does not change what those phases delivered at their original
completion points. Those records use `※` to mark statements and sections whose
interface or roadmap status this increment superseded.

## Product increment

Repository installation now uses one tool-valued option:

```text
dtr [--explain|-n] install|i [-a|--add] [-t|--tool <tool>] [<dtr-repospec>]
dtr [--explain|-n] install|i [-a|--add] [-t|--tool <tool>] [<dtr-repospec>] -- <installer-arg>...
```

The supported values are `go`, `cargo`, `uv`, `pipx`, `npm`, and `auto`.
`rust` is accepted as an alias for the preferred `cargo` value. `auto` is the
default when `--tool` is omitted. The former `--go`, `--rust`, `--cargo`,
`--uv`, `--pipx`, and `--npm` selector flags are no longer accepted.

The repository reference defaults to `.` when omitted. Bare `dtr install` and
`dtr i` therefore use the same local-repository parsing, inspection, planning,
and execution paths as an explicit `dtr install .`.

An explicit non-auto tool preserves the established backend mappings and skips
repository inspection. Native installer arguments retain the existing `--`
boundary and backend-specific safety checks.

※ `--add` originally appended successful requests to `install-all.toml`, and
the collection was managed by the top-level `install-all|ia` command with
`--list` and `--edit` action flags. The current interface stores the collection
in `kit.toml` and exposes `dtr kit install`, `dtr kit list`, and `dtr kit edit`
without compatibility aliases for the former CLI. On first non-explain use of
the default file, it silently renames `install-all.toml` only when `kit.toml` is
absent; an existing `kit.toml`, explain mode, and explicit `--file` paths do not
trigger migration.

The current command hierarchy has visible shorthand aliases: `dtr k` for
`dtr kit`, `dtr k i` for `dtr kit install`, and `dtr k ls` for `dtr kit list`.

`--add` retains normal install planning and execution, then appends the exact
successful request to the default `kit.toml`. Local paths are stored canonically;
Windows storage uses native separators without the `\\?\` prefix. Auto selection
is replaced by the resolved backend, exact duplicate entries are ignored, and
existing file text is preserved. Explain mode reports the target without
mutating it; failed installs are not tracked.

※ `dtr kit install` originally always retained native child output. It now
accepts a counted `-q`/`--quiet`: one occurrence suppresses installer stdout and
two suppress installer stderr as well; a third also disables dtr narration.
Warnings and errors remain visible at every level. With no quiet option, native
child output and immediate narration remain visible while jobs run. Dtr command,
install-success, and warning messages are replayed in configuration order after
all jobs complete. PATH warnings are replayed even when narration is disabled.
Windows narration normalizes displayed local paths to native backslash-separated
paths without the canonicalization-only `\\?\` prefix; execution argv retains
the original resolved path.

The first Ctrl-C stops new job dispatch while allowing active child processes to
finish normally. Dtr emits the accumulated replay only after they finish,
immediately before exiting 130. A second Ctrl-C terminates the active child
process groups, then still joins workers and performs the final replay. A third
Ctrl-C forces an immediate status-130 exit without replay.

Remote install sources may carry an install-only Go version query suffix, such
as `owner/repo@v1.2.3` or
`https://github.com/owner/repo@feature-branch`. Dtr separates the query from the
base `RepoSpec`; clone parsing and explicit local paths retain their existing
meaning.

## Conservative automatic selection

※ The original increment considered only exact manifests in the specified local
directory or remote repository root. Current behavior retains that rule, with
the bounded local Go-source exception described below.

Auto mode considers exact, file-like names in the repository root:

| Root manifest | Ecosystem | Selected tool |
|---|---|---|
| `go.mod` | Go | `go` |
| `Cargo.toml` | Rust | `cargo` |
| `pyproject.toml`, `setup.py`, or `setup.cfg` | Python | `uv` if available, otherwise `pipx` |
| `package.json` | JavaScript | `npm` |

Names are case-sensitive. A directory with a manifest-like name is not evidence.
Lockfiles, tool configuration, and other inferred signals are deliberately
ignored.

For a local directory with no supported manifest, a direct non-test `*.go` file
triggers one additional check. Dtr asks Git for the enclosing worktree root and
walks upward, stopping at that root, for the nearest file-like `go.mod`. A match
selects Go but does not change the requested install directory. This lets Go
apply its native ancestor-module resolution to command directories without
classifying unrelated subdirectories or changing remote inspection.

Exactly one ecosystem must be recognized. If no ecosystem or multiple
ecosystems are present, dtr declines to install and suggests an explicit
`--tool <go|cargo|uv|pipx|npm>` choice. A Python repository also declines when
neither uv nor pipx is available.

There is no ecosystem priority order. Refusing mixed evidence prevents an
arbitrary backend choice in polyglot repositories and monorepos.

## Go version queries

The suffix after `@` is passed to Go as its native version query. Go owns the
meaning and validation of versions, version prefixes, branches, tags, revisions,
and special queries. Without an explicit query, dtr retains its `@latest`
default or omits the suffix when `--no-latest` is present.

The query is removed before auto inspection, which continues to inspect the
base repository's default branch. A query does not count as Go ecosystem
evidence. If auto selects another backend, or the user explicitly selects a
non-Go backend, dtr rejects the query before invoking that tool. An explicit
query also conflicts with `--no-latest`.

Query extraction applies to URL, forge shorthand, bare GitHub, generic Git, and
SCP-like remote forms. It does not reinterpret `@` in an explicit local path or
the username portion of an SSH remote.

## Root inspection

※ Local repositories were originally inspected only with a direct directory
listing. They now also support the bounded Go ancestor lookup described above.
Remote inspection still uses the lightest supported mechanism for the repospec:

1. A recognized GitHub repository uses `gh api` to request the default branch's
   root tree. Process-scoped GitHub account selection is resolved before the
   request and retained for backend planning without a second account lookup.
2. A recognized GitLab repository uses `glab api` to list the default branch's
   root tree, including paginated responses.
3. If a forge API is unavailable, fails, returns malformed data, or reports a
   truncated tree, dtr falls back to Git.
4. Other remote Git repospecs go directly to the Git inspection path.

The Git path creates a temporary filtered, depth-one, single-branch clone with
no checkout, then reads the root tree from `HEAD`. It does not fall back to an
ordinary full checkout or repository-history clone. Temporary inspection state
is removed when discovery finishes.

If every applicable inspection path fails, dtr reports the discovery failures
and suggests the explicit tool values instead of guessing.

## Explain and authentication behavior

`--explain` still performs the read-only discovery needed to choose a backend,
but does not execute the final installer. Its output retains the existing plan
shape and names only the selected backend and final command; discovery commands
are not rendered as additional operations.

For an allowlisted explicit GitHub owner, the same process-scoped token
selection is used by GitHub API inspection, filtered Git fallback, and final
Cargo, uv, pipx, or npm Git fetching. Go retains its established import-path
installation behavior and receives no dtr-managed Git authentication. No active
GitHub CLI account or persistent Git configuration is changed, and token-derived
values remain absent from explain output and diagnostics.

## Implementation map

```text
src/cli.rs             InstallTool values, default, alias, and argument shape
src/install_detect.rs  marker inference and local/API/Git root inspection
src/kit.rs             successful-install tracking and kit management
src/resolve.rs         one-time repospec/auth resolution and backend dispatch
src/repospec.rs        inspection-safe remote normalization
src/github_auth.rs     reusable process-scoped Git environment
src/command.rs         shared child-environment application
tests/cli.rs           PATH-isolated end-to-end detection and fallback coverage
```

`serde_json` parses forge responses. Repository names and Git tree entries
remain byte-safe where the underlying platform permits non-UTF-8 paths.

## Completed implementation checklist

1. [x] Replace backend selector flags with `-t` / `--tool` and default `auto`.
2. [x] Prefer `cargo` while accepting `rust` as a value alias.
3. [x] Infer one ecosystem from exact root manifests and decline ambiguity.
4. [x] Select uv or pipx from installed Python tooling.
5. [x] Inspect local roots without cloning.
6. [x] Inspect GitHub and GitLab roots through their forge APIs.
7. [x] Fall back to filtered, depth-one Git inspection without checkout.
8. [x] Reuse process-scoped GitHub auth across discovery and installation.
9. [x] Keep explicit tool selection inspection-free.
10. [x] Add focused unit, PATH-isolated integration, help, and error coverage.
11. [x] Update the README and roadmap documentation.
12. [x] Add `just check` as the complete local validation entry point.
13. [x] Add install-only Go version queries without changing clone semantics.
14. [x] Default an omitted install repository reference to the current directory.
15. [x] Detect local Go command subdirectories through a Git-bounded ancestor module lookup.
16. [x] ※ Track successful installs in the original install-all configuration with `--add`.
17. [x] Rename that collection to the `kit` command namespace and `kit.toml`.
18. [x] Add visible `k`, `k i`, and `k ls` aliases for the kit command hierarchy.

## Validation record

The kit-alias follow-up was validated on macOS on 2026-08-12:

- `just check` passed formatting, warnings-as-errors Clippy, all 199 nextest
  tests, locked `cargo check`, `actionlint`, and `git diff --check`.
- End-to-end coverage exercised `dtr k i` and `dtr k ls` and confirmed that the
  `k`, `i`, and `ls` aliases appear in generated help.

The kit namespace follow-up was validated on macOS on 2026-08-11:

- `just check` passed formatting, warnings-as-errors Clippy, all 195 nextest
  tests, locked `cargo check`, `actionlint`, and `git diff --check`.
- The migrated default `kit.toml` loaded successfully through `dtr kit list`.

Validated on macOS on 2026-07-23:

- `just check` passed formatting, warnings-as-errors Clippy, locked nextest,
  locked `cargo check`, `actionlint`, and `git diff --check`.
- All 132 tests passed through nextest.
- Live read-only local inspection selected Cargo for this repository.
- Live read-only GitHub inspection selected Go for `cli/cli@latest` and retained
  the query in the final command.
- The requested `yuser/reepo@some-go-stuff` example produced the exact Go query
  command under explicit read-only selection.

The GitHub Actions workflow continues to run the Rust validation gates on both
macOS and Linux. Linux validation for this post-MVP increment is not claimed in
this local record until that workflow runs successfully.

## Remaining boundaries

- Automatic package, binary, workspace, or subdirectory selection in monorepos.
- Go module paths that differ from their repository path.
- Literal `scp://` and `sftp://` staging workflows.
- Configurable shorthand forge, host, source, protocol, or account defaults.
- GitHub Enterprise, GitLab, and generic-host account selection.
- Persistent checkout-to-account binding for later fetch and push operations.
- Windows support.
- JSON output, plugins, `doctor`, update, and uninstall orchestration.
