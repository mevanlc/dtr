# dtr

`dtr` means **Do the Repo** X, where X is currently: install or clone.

It accepts the repository reference you already have, resolves what it means,
and invokes the appropriate underlying tool. It supports forge-aware cloning,
process-scoped GitHub account selection, and Rust, Go, Python, and JavaScript
tool installation from repositories on macOS, Linux, and Windows.

The first-MVP feature set is complete in dtr 0.1.0. Its phase-by-phase finish
line and completion audit are recorded in [devdocs/FIRST-MVP.md](devdocs/FIRST-MVP.md).

```console
$ dtr --dry-run clone --depth 1 -O hjr265/gittop
repospec: GitHub repository hjr265/gittop
backend:  gh
target:   hjr265--gittop
command:  gh repo clone hjr265/gittop hjr265--gittop -- --depth 1

$ dtr --dry-run install https://github.com/hjr265/gittop
repospec: GitHub repository hjr265/gittop
backend:  go
command:  go install github.com/hjr265/gittop@latest

$ dtr --dry-run install --tool cargo hjr265/gittop -- --locked
repospec: GitHub repository hjr265/gittop
backend:  cargo
command:  cargo install --git https://github.com/hjr265/gittop.git --locked

$ dtr --dry-run install --tool uv astral-sh/ruff -- --force
repospec: GitHub repository astral-sh/ruff
backend:  uv
command:  uv tool install git+https://github.com/astral-sh/ruff.git --force

$ dtr --dry-run install --tool npm owner/tool -- --force
repospec: GitHub repository owner/tool
backend:  npm
command:  npm install --global --force -- git+https://github.com/owner/tool.git

$ dtr config set github.auth.auto_switch mevanlc,mike-clark-8192
$ dtr --dry-run clone mike-clark-8192/foo
repospec: GitHub repository mike-clark-8192/foo
backend:  gh
auth:     auto-switch to mike-clark-8192 (process-scoped; active gh account unchanged)
target:   foo
command:  gh repo clone https://github.com/mike-clark-8192/foo.git
```

## Install

Release downloads go to [GitHub Releases](https://github.com/mevanlc/dtr/releases).
Extract an archive and put `dtr` on your PATH. The release workflow builds Linux (static musl),
macOS, and Windows for x64 and ARM64, plus Android ARM64 and FreeBSD ARM64.
FreeBSD is built and tested on CFARM through SSH. Android execution
is not covered by hosted CI.

After the first crates.io publication, install with `cargo install dtr --locked`.
From this checkout:

```sh
cargo install --path . --locked
```

Release tags supply the version; checkout builds report `0.0.0`. See
[RELEASING.md](RELEASING.md) for CI, publishing, verification, and recovery.

### Reinstall dtr from its checkout

`dtr install .` can rebuild and reinstall dtr itself on macOS, Linux, and
Windows. This also works through `install --add`, `kit add`, and a local dtr
entry in `kit install`.
Checkouts under your home directory also use this handoff when their kit entry
is stored as `~/...`.

Dtr builds its internal `dtr-trampoline` example for the host platform, then
runs it directly from Cargo's output directory with the original arguments and
working directory. The helper uses the same application code and performs the
normal installation. It is excluded from ordinary `cargo install`; there is
no additional installed command or temporary executable copy. Cargo's reported
artifact path is used, including when a custom target directory is configured.

Interrupting Cargo metadata lookup or helper compilation returns exit status
`130`, releases the self-install locks, and leaves the current executable usable.

Another self-install using the same build directory or running executable
fails immediately with `self-install already in progress`. On Windows, dtr
renames its running executable to `dtr.exe.old` and waits for the helper. If
the original path remains missing after the helper exits, dtr restores the
backup. A successful replacement leaves the backup for cleanup on the next
non-dry-run command; the waiting parent preserves the helper's exit status.
Dry-run mode does not build the helper or clean up backups. Automatic
trampolining currently applies to local dtr repositories, not remote sources.

Cloning requires Git. Depending on the operation, dtr also uses:

- [`gh`](https://cli.github.com/) for GitHub-aware cloning, resolving the owner
  of a bare repository name, and preferred GitHub root inspection in auto mode.
- [`glab`](https://docs.gitlab.com/cli/) for GitLab-aware cloning and preferred
  GitLab root inspection in auto mode.
- Cargo for `dtr install --tool cargo` (`rust` is a tool-value alias).
- Go for `dtr install --tool go`.
- uv for `dtr install --tool uv`.
- pipx for `dtr install --tool pipx`.
- npm for `dtr install --tool npm`.

When a forge CLI is unavailable, an owner-qualified GitHub or URL-qualified
GitLab repository falls back to `git clone`. A bare repository name requires
authenticated `gh` state because its owner is otherwise unknowable.
Remote auto detection also uses Git for its filtered inspection fallback.

## Shell completion

```text
dtr completion <bash|elvish|fish|powershell|zsh>
```

`dtr completion` writes a completion script to standard output. Redirect it to
the location loaded by your shell, for example:

```sh
dtr completion zsh > ~/.zfunc/_dtr
dtr completion fish > ~/.config/fish/completions/dtr.fish
```

## Clone

```text
dtr [--dry-run|-n] [-q...] [-v...] clone [options] <dtr-repospec> [dir]
```

Repository references are classified in a stable order:

| Input | Meaning |
|---|---|
| `repo` | your GitHub repository named `repo` |
| `owner/repo` | `github.com/owner/repo` |
| `.`, `..`, `/path`, `./path`, `../path` | local Git repository |
| `https://github.com/owner/repo` | GitHub repository |
| `git@github.com:owner/repo`, `ssh://git@github.com/owner/repo` | GitHub repository over SSH |
| `https://gitlab.com/group/repo` | GitLab repository |
| another HTTP(S), SSH, or Git URL | generic Git remote |
| `git@example.com:path/repo.git` | generic SCP-like Git remote |

An explicit `./` or `../` is therefore meaningful: `owner/repo` is GitHub,
while `./owner/repo` is a local path. Bare `.` and `..` are local paths: GitHub
reserves both, so they can never name a repository. Classification does not
change based on what happens to exist in the current directory.

An optional `#fragment` is stripped from a recognized GitHub or GitLab
repository-root reference before cloning or installation. HTTP(S) GitHub URLs
also discard these well-known browser paths and everything after them:
`actions`, `agents`, `blob`, `branches`, `commit`, `commits`, `community`,
`compare`, `deployments`, `discussions`, `edit`, `fork`, `forks`, `graphs`,
`installations`, `issues`, `milestone`, `milestones`, `network`, `new`, `pkgs`,
`pull`, `pulls`, `pulse`, `releases`, `runs`, `settings`, `tags`, `tree`, and
`wiki`. GitHub HTTP(S) query parameters are discarded too. For example,
`https://github.com/owner/repo/issues/123?notification_referrer_id=1` is treated
as `https://github.com/owner/repo`. Unknown GitHub browser subpages, GitLab query
strings, and GitLab browser pages remain errors rather than being guessed at.
Fragments and query parameters on generic Git remotes are preserved.

### Clone directory modes

When no explicit `[dir]` is supplied:

```console
$ dtr clone -O owner/repo
# clones into owner--repo

$ dtr clone -D owner/repo
# creates owner/ and clones into owner/repo
```

`-O` is also spelled `--name-owner`. For nested GitLab namespaces, `-O` joins
all components with `--`, while `-D` preserves the directory hierarchy. An
explicit `[dir]` wins over either naming mode.

### Git options

Native `git clone` options can appear in normal Git-style positions:

```sh
dtr clone --depth 1 owner/repo
dtr clone owner/repo --branch main checkout-dir
dtr clone --recursive owner/repo
```

`dtr` reads the installed `git clone -h` surface at runtime, including which
options consume values. When `gh` or `glab` is selected, those options are
placed after the forge CLI's `--` separator.

`-q`/`--quiet` and `-v`/`--verbose` control [dtr's verbosity](#verbosity)
in every position, including after the repository. They are consumed by dtr.

For a recognized GitHub repository, `-U NAME` and
`--upstream-remote-name NAME` set the upstream remote name when cloning a fork.
They are forwarded to `gh` as `--upstream-remote-name`; requesting either form
requires `gh` rather than silently falling back to Git.

### GitHub account auto-switching

If you use multiple accounts on `github.com`, configure the accounts dtr may
select automatically:

```sh
dtr config set github.auth.auto_switch mevanlc,mike-clark-8192
dtr config list
dtr config list --name-only
```

`dtr config list` prints configured `key=value` entries. `--name-only` omits
their values. An empty configuration produces no list output; all available
keys remain documented in `dtr config --help`.

When the explicit owner in `owner/repo` or a recognized GitHub HTTP(S) or SSH
reference matches an account in that allowlist, dtr obtains that account's
stored token from `gh` and supplies process-scoped authentication only to the
clone or remote-install child. It does not run `gh auth switch` or change the
active GitHub CLI account. The operation uses HTTPS because selecting a token
does not select an SSH key.

GitHub-aware clones receive the token through `GH_TOKEN`. Cargo, Python, and npm
remote installs use the Git CLI and process-scoped Git configuration containing
a URL-specific authorization header. Dtr extends any existing process-scoped
Git configuration and removes inherited `GH_TOKEN` and `GITHUB_TOKEN` from
installer children. Auto-mode root inspection reuses the same selected token
through process-scoped `GH_TOKEN` for the GitHub API or the same Git header for
its fallback. No mode writes the token to disk or includes it in command
arguments, dry-run output, or errors.

An unmatched owner and a bare repository name retain normal active-account
behavior. If an owner matches the allowlist but its stored token cannot be
retrieved, dtr fails instead of silently cloning as another account.

The setting can be inspected or removed with:

```sh
dtr config get github.auth.auto_switch
dtr config unset github.auth.auto_switch
```

`dtr config edit` opens the configuration file directly in the first configured
or available editor (`DTR_EDITOR`, `VISUAL`, `EDITOR`, `vim`, then `vi`), even
if the current configuration file is invalid. In dry-run mode, it prints the
editor command without creating directories or starting it.

Configuration is stored at `<user-home>/.config/dtr/config.toml`.
`DTR_CONFIG_DIR` overrides the containing directory. The file may contain the
account allowlist, the verbosity preference, and the [uv install
options](#backend-mappings); dtr never writes GitHub tokens to it.

Clone and install operations proceed without account auto-switching when no
configuration location can be discovered. Explicitly invalid configuration,
including an empty `DTR_CONFIG_DIR` or malformed configuration file, remains an
error.

The selected identity is scoped to the clone or install process. A resulting
checkout is not persistently bound to that identity for later `git fetch` or
`git push` operations.

## Install from a repository

```text
dtr [--dry-run|-n] [-q...] [-v...] install|i [-a|--add] [-t|--tool <tool>] [--no-latest] [--main] [<dtr-repospec>] [-- <install-arg>...]
```

`install` is deliberately repo-oriented. Dtr does not wrap the package-registry
surface that `cargo install`, `go install`, `uv tool install`, `pipx install`,
and `npm install -g` already provide well.

When `<dtr-repospec>` is omitted, it defaults to `.`, so `dtr i` and `dtr i .`
are equivalent. Automatic tool selection then inspects the current directory.

`--tool` accepts `go`, `cargo`, `uv`, `pipx`, `npm`, and `auto`. `rust` is an
alias for the preferred Cargo spelling. The default is `auto`, so these are
equivalent:

```sh
dtr install owner/tool
dtr install --tool auto owner/tool
```

### Automatic tool selection

Auto selection normally lists the specified local directory or remote repository
root and recognizes these exact file names:

| Root manifest | Candidate |
|---|---|
| `go.mod` | Go |
| `Cargo.toml` | Cargo |
| `pyproject.toml`, `setup.py`, or `setup.cfg` | Python |
| `package.json` | npm |

Manifest-like directories, case variants, lockfiles, and tool configuration are
not signals. Multiple Python manifests still identify one ecosystem. If more
than one ecosystem is present, or none is present, dtr declines to run and lists
the explicit `--tool` values instead of guessing. For an unambiguous Python
repository, auto prefers uv when it is on `PATH`, then falls back to pipx; it
fails if neither is available.

There is one bounded local exception. If the specified directory has no
supported manifest but contains a file whose case-sensitive name ends in `.go`
and not `_test.go`, dtr asks Git for the enclosing worktree root and walks upward
for the nearest `go.mod`, without crossing that root. A match selects Go while
keeping the originally specified directory as the install working directory.
The file need not be named `main.go`, and unrelated local subdirectories without
direct Go source files do not inherit the repository's Go classification.

Local directories are listed directly, with only the bounded Go ancestor lookup
described above. Recognized GitHub and GitLab repositories prefer their forge
CLI's root-tree API when `gh` or `glab` is available. Other remotes, API failures,
and truncated API results fall back to a temporary depth-one, single-branch Git
clone with no tags, no checkout, and a `blob:none` filter. Dtr reads the root tree
and removes the temporary repository; it does not deliberately fall back to a
working checkout or full history.
Inspection always uses the remote default branch. Except for the exact GitHub
SSH forms documented above, an SSH or SCP-like repospec remains generic even
when its hostname is a well-known forge.

Explicit `--tool` selection skips repository inspection entirely. This is the
escape hatch for mixed-language repositories, non-Go nested packages,
inaccessible inspection APIs, and intentional backend overrides. Auto detection
identifies the ecosystem only; the selected native installer still decides
whether the repository actually provides an installable command.

### Backend mappings

Cargo repositories map to Cargo's native source modes:

```sh
dtr install --tool cargo ./my-tool
# cargo install --path ./my-tool

dtr install --tool rust hjr265/gittop -- --locked --features color
# cargo install --git https://github.com/hjr265/gittop.git --locked --features color
```

Native Cargo package and install options follow `--` and are forwarded exactly.
Dtr rejects Cargo's `--git`, `--path`, `--registry`, and `--index` source options
there because the repospec already selects the source. SCP-like Git remotes are
converted to Cargo-compatible `ssh://` URLs; literal `scp://` and `sftp://`
staging remain parked.

Python repositories map to local package paths or `git+<URL>` VCS requirements:

```sh
dtr install --tool uv ./my-tool -- --force
# uv tool install ./my-tool --force

dtr install --tool pipx owner/my-tool -- --python=3.14 --force
# pipx install --python=3.14 --force -- git+https://github.com/owner/my-tool.git
```

Uv native arguments retain their normal tokenization after dtr's `--`. Because
current pipx can install multiple positional package specs at once, dtr accepts
only option-shaped pipx arguments and puts its one resolved source after pipx's
own `--`. Values therefore use attached forms such as `--python=3.14`; `--lock`
is rejected as a conflicting alternate source.

Three `uv tool install` options can be enabled persistently instead of being
repeated after `--`:

```sh
dtr config set uv.install.force true
dtr config set uv.install.editable true
dtr config set uv.install.reinstall true

dtr install --tool uv ./my-tool
# uv tool install --force --editable --reinstall ./my-tool
```

Each key is a boolean and defaults to false. Enabled options are placed before
the resolved source in `force`, `editable`, `reinstall` order, ahead of anything
forwarded after `--`. They apply to every uv install, including those run by
`dtr kit install`, and no other backend reads them. `--editable` is a uv option
for source directories; a remote install that receives it fails in uv rather
than in dtr.

npm repositories map to local paths or npm Git package sources, then install
globally:

```sh
dtr install --tool npm ./my-tool -- --prefix=/opt/npm
# npm install --global --prefix=/opt/npm -- ./my-tool

dtr install --tool npm owner/my-tool -- --force
# npm install --global --force -- git+https://github.com/owner/my-tool.git
```

Current npm accepts multiple positional package specs, so dtr accepts only
option-shaped npm arguments and puts its one resolved source after npm's own
`--`. Option values use attached forms such as `--prefix=/opt/npm`. Dtr rejects
forwarded global-mode options because repository installs through this backend
are always global. npm owns package metadata, binary selection, dependencies,
and lifecycle scripts; install only repositories you trust. Generic HTTP(S) and
SSH remotes use npm's `git+` prefix; its native `git://` transport remains
unprefixed because npm rejects `git+git://`.

Remote Go repository installs receive `@latest` by default:

```sh
dtr install --tool go https://github.com/hjr265/gittop
# go install github.com/hjr265/gittop@latest

dtr i --tool go --no-latest hjr265/gittop
# go install github.com/hjr265/gittop

dtr install https://github.com/yuser/reepo@some-go-stuff
# auto inspects yuser/reepo, selects Go, then runs:
# go install github.com/yuser/reepo@some-go-stuff
```

A remote install repospec may end in a Go version query such as a version,
branch, tag, or revision. Dtr separates the suffix before repository inspection,
so forge APIs and Git inspect the base repository. Auto mode still requires the
base repository to identify as Go; the suffix alone does not force a backend.
An explicit query is accepted only by Go and conflicts with `--no-latest`.
An `@` in an explicit local path remains part of that path, and SSH usernames
such as `git@example.com` are not mistaken for queries.

A bare name uses the current GitHub user:

```sh
dtr install --tool go my-tool
# gh supplies <current-owner>, then:
# go install github.com/<current-owner>/my-tool@latest
```

A local repository installs all of its Go commands from that directory:

```sh
dtr install --tool go ./my-tool
# logically: (cd ./my-tool && go install ./...)
```

Use `--main` to install only the primary command from a local Go repository:

```sh
cd ./my-tool
dtr kit add --main
# logically: go install .
```

Dtr first checks the repository-root package. If it is `package main`, that
package is installed even when the repository contains other commands. If the
root is not a command, dtr searches the repository and selects a sole main
package. Zero or multiple candidates produce an error before installation; an
ambiguous error lists the candidate package paths. `--main` is currently
rejected for non-Go backends and remote Go repositories. The selection is
recorded as `main = true` when the request is added to a kit.

The implementation sets the child process working directory directly; it never
constructs a shell command.

Use `-a` or `--add` to install normally and then track the successful request
in the default `kit.toml`:

```sh
dtr install --add --tool cargo ./my-tool -- --force --features color
```

Dtr validates the existing file before starting the installer and records the
request only after the installer succeeds. A new normalized repository is
appended; an existing entry for the same normalized repository is silently
updated in place, including its tool and arguments. Unchanged comments and
surrounding formatting remain intact. Local paths are
canonicalized; Unix paths below the user home directory are stored with `~/`,
while Windows paths are stored without a `\\?\` prefix and use `\` separators;
an automatically selected backend is recorded explicitly so later batch runs
repeat the same installer choice. When dtr can prove that a Git-based install
used one stable revision, it also records the full commit and a SHA-256
fingerprint of the install request. `--dry-run` reports the file that would be
updated without installing or writing it.

## Manage an installation kit

```text
dtr [--dry-run|-n] [-q...] [-v...] kit|k add|a [--no-install] [-t|--tool <tool>] [--no-latest] [--main] [<dtr-repospec>] [-- <install-arg>...]
dtr [--dry-run|-n] [-q...] [-v...] kit|k remove|rm [--file <FILE>] [<dtr-repospec>]
dtr [--dry-run|-n] [-q...] [-v...] kit|k install|i [-j|--jobs <n|auto>] [--force] [--file <FILE>]
dtr kit|k list|ls [--file <FILE>]
dtr [--dry-run|-n] kit|k edit|e [--file <FILE>]
```

`k` is a visible alias for `kit`; within that namespace, `i` aliases `install`,
`ls` aliases `list`, `a` aliases `add`, `rm` aliases `remove`, and `e` aliases
`edit`. The aliases can be combined as `dtr k a`, `dtr k rm`, `dtr k i`,
`dtr k ls`, and `dtr k e`.

`kit add` installs one repository and then tracks it, so `dtr kit add <spec>`
is equivalent to `dtr install --add <spec>`. Its repository, tool, Go-version,
primary-command, and native-argument options match `dtr install`; the repository
defaults to the current directory. Add `--no-install` to track the repository
without invoking its installer; `-i`/`--install` states the default explicitly
and conflicts with `--no-install`. Dry-run mode reports the kit file without
installing or changing it.

`kit remove` deletes the matching entry and its generated install state from the
kit; installed binaries and repository files stay in place. The repository
defaults to the current directory, so `dtr k rm` stops tracking the current
checkout. It uses the same normalized repository identity as `kit add`, accepts
`~/` paths even for deleted checkouts, and needs no installer or network access.
For example, `dtr kit remove owner/tool` also matches an entry stored as
`https://github.com/owner/tool.git`. An untracked repository is an error and
leaves the kit unchanged. Dry-run mode prints the entry it would remove without
writing the file. Changes are atomic and preserve other entries and their comments.

`kit install` reads `<user-home>/.config/dtr/kit.toml` and plans each
`[[install]]` entry in file order. `DTR_CONFIG_DIR` overrides the containing
directory just as it does for `config.toml`.

Before installing, dtr checks any generated `installed` state. If both the
request fingerprint and selected Git commit still match, the entry is reported
as up to date and skipped. Use `-v` to report each skipped entry once in the
final ordered replay, or `-vv` to also explain cache decisions.
`--force` bypasses every cached skip, runs the installers, and refreshes the
resulting state. `dtr kit install -n` lists only entries that would be installed,
updated, or rebuilt, one per line in kit order:

```text
install: ~/p/my/dtr (cargo)
install: owner/tool (go)
```

Add `-v` to include `skip: ...` lines for up-to-date entries and print the job
count and exact commands to stderr. With `--force`, every entry is listed for
installation. Dry runs never invoke installers or write install state.

Local repositories are cacheable only when the whole worktree is clean before
and after installation and remains at the same commit. Tracked changes,
untracked files, and dirty submodules prevent caching; ignored files do not.
Remote Cargo installs support the default HEAD, explicit branches, peeled tags,
and revisions that can be mapped unambiguously to a full object ID. Remote uv,
pipx, and npm installs track the default HEAD. Versioned Go installs use Go's
module-origin metadata only when it identifies an exact Git commit. Mutable
remote selections must resolve to the same commit before and after installation.
Other sources continue to install normally without generated state.

Git or module probe failures produce a warning and fall back to installation.
They do not turn a successful installer result into a failure. A successful
install that cannot be tied safely to a commit clears stale state rather than
risking a false skip.

On the first non-dry-run operation that uses the default file, dtr silently
renames a sibling `install-all.toml` to `kit.toml` when `kit.toml` does not yet
exist. If both files exist, dtr uses `kit.toml` and leaves `install-all.toml`
untouched. Dry-run mode and explicit `--file` paths do not perform this migration.

`--file FILE` selects an alternate kit TOML file for remove, install, list, or edit.
`kit list` prints each tracked entry as a shell-safe, reusable
`dtr install ...` command without invoking an installer; generated install state
is not included in that command. An empty kit lists no commands, including after
removing its last entry. `kit edit` launches
the first configured or available editor in this order:
`DTR_EDITOR`, `VISUAL`, `EDITOR`, `vim`, then `vi`. Environment-provided editor
commands may contain quoted arguments, such as `DTR_EDITOR='code --wait'`. Dtr
creates the containing directory before launching the editor. In dry-run mode,
it prints the editor command without creating directories or starting it.

`--jobs N` runs at most that many installs concurrently and requires a positive
integer. The default, `--jobs auto`, uses half of the available CPU cores rounded
up: `ceil(cores / 2)`, with a minimum of one job. Entries enter the bounded work
queue in configuration order, while process starts, native installer output, and
completion order can differ. Native output can therefore interleave when more
than one job runs.

`-q` or `--quiet` hides routine native installer output and prints compact
results once in the final ordered replay. `-qq` also hides those results.
Dtr's warnings and errors remain visible at every level. Hidden child stdout
and stderr are retained and printed to stderr if that child fails.

Each entry requires `repospec`. It accepts the same repository references as
`dtr install`, plus a leading `~/` for a path below the user home directory.
`tool` defaults to `auto` and accepts the normal install tool values. `no_latest`
and `main` default to `false`; `main = true` requests the local Go primary-command
selection described above. `args` is an array of native installer arguments with
the same validation as arguments following `dtr install ... --`. A kit may have
only one entry for each normalized repository; equivalent local paths and remote
repository spellings count as the same repository. After successful installs,
dtr atomically refreshes eligible entries in the selected default or `--file`
kit while preserving comments and surrounding formatting. If the entry was
edited while its installer ran, dtr warns and leaves it untouched.

For example, this rebuilds one Cargo repository with its default features and
ripgrep with PCRE2 enabled:

```toml
[[install]]
repospec = "~/p/my/dtr"
tool = "cargo"
args = ["--force"]
installed = { commit = "0123456789abcdef0123456789abcdef01234567", branch = "main", request = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }

[[install]]
repospec = "~/p/my/ripgrep"
tool = "cargo"
args = ["--force", "--features", "pcre2"]
```

`installed` is generated state. Its `commit` is the exact Git object installed;
`branch` records the active branch name (if not in a detached HEAD state) for
informational purposes; `dirty = true` indicates that uncommitted changes were
present at install time and prevents future kit install runs from skipping
rebuilds until installed from a clean worktree; `request` covers the repository,
tool, Go-version and primary-command behavior, native arguments, and relevant
dtr-managed installer options. Editing any of those inputs causes the next kit
run to install again and replace the state. Removing `installed` also forces
that entry's next normal installation.

Malformed or missing configuration is a fatal error. Once the file has been
loaded, an entry that cannot be planned, started, or completed emits a warning;
dtr continues with the remaining entries and exits with status 1 after any such
failure. Successful entries still refresh their state when another entry fails.
Native installer output is visible at verbosity 0 and above.
At normal verbosity and above, dtr progress and result messages remain visible
while jobs run and are replayed in configuration order after all jobs complete.
Command echoes join that replay at `-v`. PATH warnings are included at every level.
On the first Ctrl-C, dtr stops dispatching queued entries while allowing active
children to finish normally. Only after they finish does dtr replay the messages
generated so far, immediately before exiting with status 130. A second Ctrl-C
terminates the active child process groups, then still waits for worker cleanup
and performs the final replay. A third Ctrl-C exits immediately without replay.

## Verbosity

Use repeatable `-q`/`--quiet` and `-v`/`--verbose` on any command:

| Level | Flags | Output |
|---|---|---|
| -2 | `-qq` | Dtr warnings and failure diagnostics |
| -1 | `-q` | Compact results; no progress or routine native tool output |
| 0 | none | Progress, results, and normal native tool output |
| 1 | `-v` | Also exact commands, working directories, and skipped kit entries |
| 2 | `-vv` | Also repository resolution, backend selection, and kit cache decisions |

Counts add across command positions and cancel: `-vvq` is level 1, and
`dtr -q kit -v install` is level 0. Dtr adds all counts before capping the
result to -2 through 2. This makes aliases adjustable:

```sh
alias dtrc='dtr clone -q'
dtrc owner/repo       # level -1
dtrc -v owner/repo    # level 0
dtrc -vv owner/repo   # level 1
```

The default baseline is 0. For a persistent preference, set
`dtr config set verbosity -1`; flags then adjust that baseline. For example,
with that setting, `-v` returns to level 0 and `-vv` selects level 1.

Dtr's diagnostic output goes to stderr. Command results remain available at
every verbosity: clone paths, configuration values, kit listings, completion
scripts, and requested dry-run plans. Installer arguments after `--` go to the
installer unchanged; for example, `dtr install --tool cargo . -- -vv` makes
Cargo verbose independently of dtr.

Quiet modes retain child stdout and stderr in temporary files and reveal them
on stderr if a child fails, preserving the exit status. Native warnings mixed
into a successful child's output are hidden too; dtr's own warnings remain
visible. Editor sessions keep their normal interactive input and output.

At `-v`, individual clone and install operations print the command about to run.
On Windows, displayed local paths use native backslash separators and omit the
verbatim `\\?\` prefix that filesystem canonicalization may produce.
For a command that runs in a repository directory, the command line includes
its absolute working directory:

```console
→ go install ./... (in /Users/me/src/tool)
```

After a successful clone, dtr prints the new repository's absolute path as its
only stdout line. This keeps the path easy to copy and makes command substitution
useful:

```sh
cd "$(dtr clone -q owner/repo)"
```

After a successful Go install, dtr asks Go for the installed command targets and,
at level -1 or above, reports every binary and its directory. Local installs use `go list`; remote
installs use `go env GOBIN GOPATH` and the installed import path:

```console
installed: gh, gen-docs → /Users/me/.go/bin
warning: 'gh' is shadowed by /opt/homebrew/bin/gh (earlier on PATH)
```

Dtr warns when a reported Go binary directory is absent from `PATH`, or when an
earlier executable shadows an installed binary. A PATH entry that is a symlink
to the installed binary is not treated as a shadow. `dtr kit install` also repeats
these warnings in its final replay so they remain visible after native output.

## Dry run before doing

Use `-n` or `--dry-run` to resolve the complete operation without performing it.
Install, kit, and config subcommands inherit the flag, so it can appear before,
between, or after their command names:

```sh
dtr -n clone -D owner/repo
dtr --dry-run install owner/repo
dtr --dry-run install --tool cargo owner/repo -- --locked
dtr --dry-run install --tool uv owner/repo -- --force
dtr --dry-run install --tool npm owner/repo -- --force
dtr -n kit edit
dtr kit -n edit
dtr kit edit -n
```

The three kit examples are equivalent; `--dry-run` and the `k`/`e` aliases work
in the same positions. Flags after the installer argument separator `--` are
passed to the installer.

For clone, put the dry-run flag before `clone`. Git already uses `clone -n`
for `--no-checkout`, so both meanings remain available:

```sh
dtr -n clone owner/repo  # dry run
dtr clone -n owner/repo  # clone without checkout
```

Dry-run mode uses the same typed command plan as execution. It may perform
read-only discovery, such as resolving the current user, querying a forge API,
or creating and removing an inspection-only temporary Git clone. It never starts
the resolved clone/install operation or creates its planned target directories.

## Current boundaries

- Well-known forge handling covers `github.com` and `gitlab.com`.
- Well-known HTTP(S) GitHub browser paths are stripped to the repository root;
  unknown GitHub browser paths and GitLab browser pages such as `/-/blob/` are
  rejected rather than guessed at. Fragments on repository-root references are
  stripped, as are query parameters on GitHub HTTP(S) URLs.
- Literal `scp://` and `sftp://` staging is parked for a later milestone.
- Go module paths that differ from their repository paths, explicit package or
  workspace selection in monorepos beyond local Go `--main`, and GitLab/Enterprise
  account selection are later work.

The overall finish line and current roadmap status live in
[devdocs/FIRST-MVP.md](devdocs/FIRST-MVP.md). The current automatic install
selection design is recorded in
[devdocs/PLAN-AUTO-INSTALL.md](devdocs/PLAN-AUTO-INSTALL.md). Historical
first-MVP design records and acceptance criteria live in
[devdocs/PLAN-MVP00.md](devdocs/PLAN-MVP00.md),
[devdocs/PLAN-MVP01.md](devdocs/PLAN-MVP01.md),
[devdocs/PLAN-MVP02.md](devdocs/PLAN-MVP02.md),
[devdocs/PLAN-MVP03.md](devdocs/PLAN-MVP03.md), and
[devdocs/PLAN-MVP04.md](devdocs/PLAN-MVP04.md).
Within those historical records, `※` marks statements and sections superseded
by the current install interface or roadmap status.

## Development

```sh
just check
```

This runs formatting, Clippy, nextest, doctests, `cargo check`, release helper
tests, `actionlint`, and `git diff --check`.

## License

[MIT](LICENSE).
