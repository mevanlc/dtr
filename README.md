# dtr

`dtr` means **do/develop the right repo**.

It accepts the repository reference you already have, resolves what it means,
and invokes the appropriate underlying tool. It supports forge-aware cloning,
process-scoped GitHub account selection, and Rust and Go tool installation from
repositories on macOS and Linux.

```console
$ dtr --explain clone --depth 1 -O hjr265/gittop
repospec: GitHub repository hjr265/gittop
backend:  gh
target:   hjr265--gittop
command:  gh repo clone hjr265/gittop hjr265--gittop -- --depth 1

$ dtr --explain install --go https://github.com/hjr265/gittop
repospec: GitHub repository hjr265/gittop
backend:  go
command:  go install github.com/hjr265/gittop@latest

$ dtr --explain install --rust hjr265/gittop -- --locked
repospec: GitHub repository hjr265/gittop
backend:  cargo
command:  cargo install --git https://github.com/hjr265/gittop.git --locked

$ dtr config set github.auth.auto_switch mevanlc,mike-clark-8192
$ dtr --explain clone mike-clark-8192/foo
repospec: GitHub repository mike-clark-8192/foo
backend:  gh
auth:     auto-switch to mike-clark-8192 (process-scoped; active gh account unchanged)
target:   foo
command:  gh repo clone https://github.com/mike-clark-8192/foo.git
```

## Install

From this checkout:

```sh
cargo install --path .
```

Cloning requires Git. Depending on the operation, dtr also uses:

- [`gh`](https://cli.github.com/) for GitHub-aware cloning and resolving the
  owner of a bare repository name.
- [`glab`](https://docs.gitlab.com/cli/) for GitLab-aware cloning.
- Cargo for `dtr install --rust` or its `--cargo` alias.
- Go for `dtr install --go`.

When a forge CLI is unavailable, an owner-qualified GitHub or URL-qualified
GitLab repository falls back to `git clone`. A bare repository name requires
authenticated `gh` state because its owner is otherwise unknowable.

## Clone

```text
dtr [--explain|-n] clone [options] <dtr-repospec> [dir]
```

Repository references are classified in a stable order:

| Input | Meaning |
|---|---|
| `repo` | your GitHub repository named `repo` |
| `owner/repo` | `github.com/owner/repo` |
| `/path`, `./path`, `../path` | local Git repository |
| `https://github.com/owner/repo` | GitHub repository |
| `https://gitlab.com/group/repo` | GitLab repository |
| another HTTP(S), SSH, or Git URL | generic Git remote |
| `git@example.com:path/repo.git` | generic SCP-like Git remote |

An explicit `./` or `../` is therefore meaningful: `owner/repo` is GitHub,
while `./owner/repo` is a local path. Classification does not change based on
what happens to exist in the current directory.

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
dtr clone -vq owner/repo
```

`dtr` reads the installed `git clone -h` surface at runtime, including which
options consume values. When `gh` or `glab` is selected, those options are
placed after the forge CLI's `--` separator.

### GitHub account auto-switching

If you use multiple accounts on `github.com`, configure the accounts dtr may
select automatically:

```sh
dtr config set github.auth.auto_switch mevanlc,mike-clark-8192
```

When the explicit owner in `owner/repo` or a GitHub URL matches an account in
that allowlist, dtr obtains that account's stored token from `gh` and supplies it
only to the clone or Rust-install child process. It does not run `gh auth switch`
or change the active GitHub CLI account. The operation uses HTTPS because
selecting a token does not select an SSH key.

GitHub-aware clones receive the token through `GH_TOKEN`. Rust installs use
Cargo's Git-CLI mode and process-scoped Git configuration containing a
URL-specific authorization header. Dtr extends any existing process-scoped Git
configuration and removes inherited `GH_TOKEN` and `GITHUB_TOKEN` from the Cargo
child. Neither mode writes the token to disk or includes it in command arguments,
explain output, or errors.

An unmatched owner and a bare repository name retain normal active-account
behavior. If an owner matches the allowlist but its stored token cannot be
retrieved, dtr fails instead of silently cloning as another account.

The setting can be inspected or removed with:

```sh
dtr config get github.auth.auto_switch
dtr config unset github.auth.auto_switch
```

Configuration is stored in `$XDG_CONFIG_HOME/dtr/config.toml`, or
`$HOME/.config/dtr/config.toml` when `XDG_CONFIG_HOME` is unset.
`DTR_CONFIG_DIR` overrides the containing directory. The file contains account
names only; dtr never writes GitHub tokens to it.

The selected identity is scoped to the clone or install process. A resulting
checkout is not persistently bound to that identity for later `git fetch` or
`git push` operations.

## Install from a repository

```text
dtr [--explain|-n] install|i --go [--no-latest] <dtr-repospec>
dtr [--explain|-n] install|i <--rust|--cargo> <dtr-repospec> [-- <cargo-install-arg>...]
```

`install` is deliberately repo-oriented. Dtr does not wrap the package-registry
surface that `cargo install`, `go install`, `uv tool install`, `pipx install`,
and `npm install -g` already provide well.

Rust repositories map to Cargo's native source modes:

```sh
dtr install --rust ./my-tool
# cargo install --path ./my-tool

dtr install --cargo hjr265/gittop -- --locked --features color
# cargo install --git https://github.com/hjr265/gittop.git --locked --features color
```

`--rust` is the primary ecosystem spelling; `--cargo` is a visible alias.
Native Cargo package and install options follow `--` and are forwarded exactly.
Dtr rejects Cargo's `--git`, `--path`, `--registry`, and `--index` source options
there because the repospec already selects the source. SCP-like Git remotes are
converted to Cargo-compatible `ssh://` URLs; literal `scp://` and `sftp://`
staging remain parked.

Remote Go repository installs receive `@latest` by default:

```sh
dtr install --go https://github.com/hjr265/gittop
# go install github.com/hjr265/gittop@latest

dtr i --go --no-latest hjr265/gittop
# go install github.com/hjr265/gittop
```

A bare name uses the current GitHub user:

```sh
dtr install --go my-tool
# gh supplies <current-owner>, then:
# go install github.com/<current-owner>/my-tool@latest
```

A local repository installs all of its Go commands from that directory:

```sh
dtr install --go ./my-tool
# logically: (cd ./my-tool && go install ./...)
```

The implementation sets the child process working directory directly; it never
constructs a shell command.

## Explain before doing

Put `-n` or `--explain` before the command to resolve the complete operation
without performing it:

```sh
dtr -n clone -D owner/repo
dtr --explain install --go owner/repo
dtr --explain install --rust owner/repo -- --locked
```

The position is intentional. Git already uses `clone -n` for `--no-checkout`,
so both meanings remain available:

```sh
dtr -n clone owner/repo  # explain
dtr clone -n owner/repo  # clone without checkout
```

Explain mode uses the same typed command plan as execution. It may perform
read-only discovery, such as resolving the current user with `gh`, but it never
starts the resolved clone/install operation or creates planned directories.

## Current boundaries

- Well-known forge handling covers `github.com` and `gitlab.com`.
- Forge browser-page URLs such as `/tree/` and `/-/blob/` are rejected rather
  than guessed at.
- Literal `scp://` and `sftp://` staging is parked for a later milestone.
- Go module paths that differ from their repository paths, automatic package
  selection in monorepos, uv/pipx/npm repository installs, GitLab/Enterprise
  account selection, and Windows are later work.

The overall finish line and phase status live in
[devdocs/FIRST-MVP.md](devdocs/FIRST-MVP.md). Detailed design records and
acceptance criteria live in [devdocs/PLAN-MVP00.md](devdocs/PLAN-MVP00.md),
[devdocs/PLAN-MVP01.md](devdocs/PLAN-MVP01.md), and
[devdocs/PLAN-MVP02.md](devdocs/PLAN-MVP02.md).

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo nextest run --locked
cargo check --locked
actionlint .github/workflows/ci.yml
git diff --check
```
