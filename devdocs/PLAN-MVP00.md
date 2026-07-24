# dtr MVP00 plan

Status: implemented and validated on macOS and Linux (2026-07-23)

Roadmap: [dtr first MVP](FIRST-MVP.md), phase 1 of 5

※ Historical note: this document records the interface and scope delivered by
MVP00. Its `--go` spelling and statements that other backends or automatic
selection are future work are intentionally preserved as phase history. The
current install interface and the completed automatic-selection increment are
documented in [PLAN-AUTO-INSTALL.md](PLAN-AUTO-INSTALL.md).

A marked statement or section contains interface or roadmap status that has
since been superseded.

## Product definition

`dtr` means **do/develop the right repo**.

The important noun is _repo_. `dtr` smooths over the inconsistent local-path,
Git-remote, and forge-aware surfaces of development tools. It does not replace
the package registries or the already-good package-spec interfaces of Cargo,
Go, uv, pipx, or npm.

The guiding behavior is:

1. Accept the repository reference the user naturally has.
2. Classify it deterministically and without guessing from network failures.
3. Select the most capable applicable tool.
4. Show the resolution clearly when asked.
5. Invoke the tool with an argv vector, never through a shell command string.

## MVP00 scope

MVP00 delivers the common repository resolver and two end-to-end operations:

- `dtr clone`, covering the supported repository-reference grammar.
- ※ `dtr install --go`, proving that the same reference can drive a non-clone
  repo operation.
- `dtr -n` / `dtr --explain`, resolving and printing without performing the
  final operation.
- macOS and Linux support.

MVP00 intentionally does not include:

- Registry package installation. Use `cargo install <crate>`,
  `go install <package>`, `uv tool install <package>`,
  `pipx install <package>`, or `npm install -g <package>` directly.
- ※ Automatic installer selection.
- ※ Rust/Cargo, uv, pipx, or npm repo-install backends.
- `scp://` or `sftp://` staging through a temporary directory.
- Configuration files, a plugin system, `doctor`, JSON output, update, or
  uninstall operations.
- Windows support.

## Usage as the functional requirements document

※ The install form below is the phase-era interface; the clone form remains
current.

```text
dtr [--explain|-n] clone [clone-options] <dtr-repospec> [dir]
dtr [--explain|-n] install|i --go [--no-latest] <dtr-repospec>
```

`--explain` is a pre-command option on purpose. `git clone` already defines
`-n` as `--no-checkout`, so both of these remain meaningful and unambiguous:

```text
dtr -n clone owner/repo       # explain what dtr would run
dtr clone -n owner/repo       # really clone with git's --no-checkout
```

### Repository specifications

```text
dtr clone <reponame>
```

Clone a repository owned by the current GitHub CLI-authenticated user. GitHub
is the fixed shorthand forge in MVP00. The default forge, host, and account
become configurable after MVP00.

```text
dtr clone <owner>/<reponame>
```

Clone the named GitHub repository. This shorthand also defaults to GitHub in
MVP00.

```text
dtr clone /<path>
dtr clone ./<path>
dtr clone ../<path>
```

Clone a local Git repository from an absolute or explicitly relative path.
Unprefixed filesystem-looking strings do not become local paths merely because
they currently exist: `owner/repo` always retains its forge-shorthand meaning.

```text
dtr clone http[s]://<well-known-forge>/<namespace>/<reponame>[.git]
```

Recognize GitHub and GitLab URLs and prefer their forge CLIs. MVP00 recognizes
`github.com` and `gitlab.com`; configurable and self-managed forge hosts are
parked. GitLab namespaces may contain subgroups.

```text
dtr clone http[s]://<hostname>/<path>
dtr clone ssh://<hostname>/<path>
dtr clone git://<hostname>/<path>
dtr clone <user>@<hostname>:<path>
```

Pass generic Git transports to `git clone`. An SCP-like Git remote such as
`git@example.com:owner/repo.git` is supported; literal `scp://` and `sftp://`
inputs are not part of MVP00.

Only exact repository-root forge URLs are accepted as forge references.
Browser page URLs containing `/tree/`, `/blob/`, query strings, or fragments
produce a focused error instead of silently cloning or installing a different
repository than the user intended.

### Clone options

`dtr clone` owns these options:

```text
-O, --name-owner  when [dir] is omitted, clone as <namespace>--<repo>
-D                when [dir] is omitted, clone as <namespace>/<repo>
```

The options are mutually exclusive and apply only to recognized forge repos.
For GitHub, they preserve the existing `gcl` meanings exactly:

- `-O` maps `owner/repo` to `owner--repo`.
- `-D` maps `owner/repo` to `owner/repo` and creates the parent directory.

For a nested GitLab namespace, each namespace separator becomes `--` under
`-O`, while `-D` retains the namespace directory structure. An explicit `[dir]`
wins over either derived-name option.

Recognized `git clone` options may appear in normal `git clone`-style positions.
`dtr` obtains the current option/arity surface from `git clone -h`, using a
Rust implementation of the proven `gcl` parsing contract. When the selected
backend is `gh` or `glab`, Git options are placed after the backend's `--`.

### ※ Go repo installation

```text
dtr install --go <dtr-repospec>
dtr i --go <dtr-repospec>
```

`install` always means _install from a repository_. It does not accept Go
registry/package-spec input as a separate namespace; users should continue to
invoke `go install` directly for that already-good surface.

For a remote repository, dtr derives the Go import path and appends `@latest`:

```text
dtr install --go https://github.com/hjr265/gittop
# go install github.com/hjr265/gittop@latest
```

`--no-latest` suppresses the addition:

```text
dtr install --go --no-latest https://github.com/hjr265/gittop
# go install github.com/hjr265/gittop
```

This deliberately exposes Go's normal current-module behavior and possible
failure when no version is supplied. In other words: “Late but latest.”
—Rajinikanth

MVP00 derives remote Go import paths without cloning or inspecting the repo:

- A bare repo name obtains the current owner from authenticated `gh` state and
  becomes `github.com/<current-owner>/<repo>`.
- `owner/repo` becomes `github.com/owner/repo`.
- A recognized forge URL becomes `<host>/<namespace>/<repo>`.
- A generic HTTP(S), SSH, or SCP-like remote becomes `<host>/<path>` when that
  conversion is unambiguous.

If a remote cannot be converted unambiguously, dtr reports that limitation
instead of passing a guessed import path to Go. A repository whose declared Go
module path differs from its forge path is outside MVP00 and will return Go's
native diagnostic.

For a local repository, the planned command is logically:

```text
(cd <repo> && go install ./...)
```

Implementation uses `std::process::Command::current_dir`; it does not invoke a
shell. `--no-latest` has no effect on a local repo and is rejected there so a
mistake is not silently ignored.

MVP00 installs remote repository-root Go commands only. Discovering a module
path that differs from its repository URL, choosing among multiple commands,
and selecting a package below the repository root are later design work.

※ The `--go` selector is required in MVP00 even though it is the only installer.
Leaving the selector out is reserved for later repository inspection and
automatic backend selection.

## Deterministic repospec resolution

Parse in this order:

1. Absolute local path beginning `/`.
2. Explicit relative local path beginning `./` or `../`.
3. `http://` or `https://` URL for a recognized forge.
4. Generic `http://`, `https://`, `ssh://`, or `git://` Git URL.
5. SCP-like Git remote matching `[user@]host:path`.
6. GitHub shorthand containing one slash: `owner/repo`.
7. Bare GitHub repository name.
8. Otherwise, a focused unsupported-repospec error.

This order is part of the public contract. Resolution must not depend on
whether a coincidentally named local file exists.

The normalized model should retain meaning instead of reducing everything to a
string too early. An indicative Rust shape is:

```rust
enum RepoSpec {
    Local {
        path: PathBuf,
    },
    Forge {
        forge: Forge,
        host: String,
        namespace: Vec<String>,
        repo: String,
        original: OsString,
    },
    GitUrl {
        remote: OsString,
    },
    ScpLike {
        remote: OsString,
    },
    GithubMine {
        repo: String,
    },
}

enum Forge {
    GitHub,
    GitLab,
}
```

Parsing, target-directory derivation, backend selection, and command rendering
remain separate pure operations wherever possible.

## Backend selection

### Clone

| Repospec | Preferred backend | Fallback |
|---|---|---|
| bare GitHub name | `gh repo clone` | none; `gh` supplies the authenticated owner |
| GitHub shorthand/URL | `gh repo clone` | `git clone` with a normalized GitHub URL |
| GitLab URL | `glab repo clone` | `git clone` with the original URL |
| local or generic Git remote | `git clone` | none |

Tool presence is checked through `PATH`. A missing preferred forge CLI may
select the documented fallback, but a forge CLI that starts and fails returns
its own failure; dtr does not retry through another transport and mask the
reason.

GitHub CLI accepts a bare repository name as the authenticated user's repo, and
both GitHub CLI and GitLab CLI accept Git options after `--`. The adapters
should preserve those native behaviors rather than reimplement authentication
or fork/upstream handling.

### ※ Install

MVP00 has one backend:

| Selector | Remote command | Local command |
|---|---|---|
| `--go` | `go install <derived-import-path>@latest` | `go install ./...` with `current_dir` set |

No installation backend falls back to another ecosystem.

## Explain behavior

`--explain` is an execution boundary, not a best-effort preview. It performs
argument parsing, repospec normalization, tool discovery, target derivation,
and command construction, then stops before the operation subprocess starts.

Human-readable output includes:

```text
repospec: github repository hjr265/gittop
backend:  go
command:  go install github.com/hjr265/gittop@latest
```

For clone operations it also includes the resolved target directory. Command
rendering must be shell-safe and unambiguous for spaces and non-UTF-8 Unix
arguments. Execution continues to use the original `OsString` argv, not the
rendered text.

Read-only discovery needed to resolve intent is allowed during `--explain`;
mutation is not. Explain exits successfully only when dtr could construct a
complete executable operation.

## CLI and implementation structure

Use Rust and Clap. Do not add an async runtime for MVP00.

Implemented modules:

```text
src/
  main.rs          minimal process entry point
  lib.rs           operation dispatch and explain/execute boundary
  cli.rs           Clap root parser and dtr-owned options
  repospec.rs      ordered classification and normalization
  clone_args.rs    git-clone option/arity parsing
  resolve.rs       operation-to-backend resolution
  command.rs       typed command plan, rendering, and execution
  error.rs         focused dtr diagnostics
tests/
  cli.rs           PATH-isolated end-to-end backend tests
```

Use a typed command plan:

```rust
struct CommandPlan {
    program: OsString,
    args: Vec<OsString>,
    current_dir: Option<PathBuf>,
    target_dir: Option<PathBuf>,
    preparations: Vec<PathBuf>,
    repospec: String,
    backend: &'static str,
}
```

The same `CommandPlan` must feed both explain output and actual execution. This
prevents the explained operation from drifting away from the executed one.

Use `std::process::Command` directly, inherit stdin/stdout/stderr during normal
execution, and return the child tool's exit status where the platform exposes
one. Dtr-originated resolution failures use exit status 2 and prefix diagnostics
with `dtr: error:`; Clap retains its native usage-error rendering.

Dependencies should start small:

- `clap` with derive support.
- A URL parser suitable for strict hierarchical URL validation.
- The display-only shell renderer is implemented locally so non-UTF-8 `OsStr`
  values remain unambiguous without affecting execution argv.

## Testing strategy

### Unit tests

Use table-driven tests for:

- Every repospec example and every parser-precedence boundary.
- `.git` and trailing-slash normalization.
- Rejection of forge browser-page URLs, queries, and fragments.
- GitHub and nested GitLab target derivation under default, `-O`, and `-D`.
- Go import-path derivation, default `@latest`, and `--no-latest`.
- Shell-safe explain rendering.
- Non-UTF-8 local paths and argv on Unix.

### Integration tests

Place deterministic stub executables named `git`, `gh`, `glab`, and `go` first
in a temporary `PATH`. Each records argv and working directory without touching
the network. Cover:

- Preferred backend and missing-tool fallback selection.
- Exact `gh repo clone` and `glab repo clone` argument order.
- Git options forwarded after `--` to forge CLIs.
- Exact preservation of `-O` as `owner--repo` and `-D` as `owner/repo`.
- Explain never starting the operation executable.
- Child exit-status propagation.
- Go remote and local installation command plans.

Run these validation gates for stable changes:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run
cargo check
git diff --check
```

Small opt-in live smoke tests may use public repositories, but network and
authenticated-forge tests are not part of the default suite.

## Implementation sequence

1. [x] Scaffold the Rust binary and Clap root command.
2. [x] Implement and table-test `RepoSpec` parsing.
3. [x] Implement typed `CommandPlan` plus explain rendering.
4. [x] Implement `git clone` planning/execution for local and generic remotes.
5. [x] Port the `gcl` short-help parsing contract and flexible clone option order.
6. [x] Add GitHub clone selection and exact `-O` / `-D` behavior.
7. [x] Add GitLab URL recognition and clone selection.
8. [x] ※ Add `install --go`, default `@latest`, and `--no-latest`.
9. [x] Add stubbed end-to-end tests and complete the validation gates.
10. [x] Update README examples from the verified CLI help and behavior.

## MVP00 acceptance criteria

MVP00 is complete when:

- [x] Every documented repospec is either resolved as specified or rejected with a
  focused explanation.
- [x] Bare names and `owner/repo` use the documented GitHub defaults.
- [x] Local paths never collide with forge shorthand.
- [x] GitHub and GitLab URLs choose the appropriate available forge CLI and fall
  back exactly as documented.
- [x] `-O` and `-D` preserve their literal directory-layout contracts.
- [x] Native Git clone options survive parsing and backend translation.
- [x] ※ `dtr install --go` converts supported remote repo references to Go import
  paths, adds `@latest` by default, and honors `--no-latest`.
- [x] `dtr -n ...` explains the exact `CommandPlan` and performs no mutation.
- [x] No command is executed through a shell.
- [x] The focused and full validation gates pass on macOS and Linux.

### ※ Validation record

- macOS: `cargo nextest run` passes all 43 unit and PATH-isolated integration
  tests.
- Linux: the Rust 1.97 slim container passes formatting, Clippy with warnings
  denied, all 43 tests through Cargo's built-in harness (nextest is absent in
  the clean container), and `cargo check`.
- `actionlint` passes the GitHub Actions workflow, which runs the normal nextest
  gate on both `ubuntu-latest` and `macos-latest`.
- Live macOS explain-mode checks cover authenticated bare-name resolution,
  GitHub CLI selection, GitLab fallback, local targets, and Go import paths.

## ※ Parked for MVP01+

- `dtr install [--rust|--cargo|--uv|--pipx|--npm] <repospec>`.
- Omit the install selector and inspect repository metadata to select a backend.
- Resolve monorepos, multiple binaries, and package subdirectories.
- Select an explicit Go version query or Git ref.
- `scp://` / `sftp://` staging followed by a local operation.
- Configurable default forge, host, account, and protocol.
- Self-managed GitHub and GitLab hosts.
- `dtr doctor`, machine-readable resolution output, and shell integration.
- Windows process/path behavior.

Before adding any installer backend, document its remote-repo and local-path
mapping independently. The value of dtr is highest where the base tools are
uneven; duplicating their registry-package surfaces is explicitly out of scope.

## External behavior references

- GitHub CLI `repo clone`: <https://cli.github.com/manual/gh_repo_clone>
- GitLab CLI `repo clone`: <https://docs.gitlab.com/cli/repo/clone/>
- Go installation semantics: `go help install` from the installed Go toolchain
- Git clone option surface: `git clone -h` from the installed Git toolchain
