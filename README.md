# dtr

`dtr` means **do/develop the right repo**.

It accepts the repository reference you already have, resolves what it means,
and invokes the appropriate underlying tool. MVP00 supports forge-aware cloning
and Go tool installation from repositories on macOS and Linux.

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

## Install from a repository

```text
dtr [--explain|-n] install|i --go [--no-latest] <dtr-repospec>
```

`install` is deliberately repo-oriented. Dtr does not wrap the package-registry
surface that `cargo install`, `go install`, `uv tool install`, `pipx install`,
and `npm install -g` already provide well.

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
- Literal `scp://` and `sftp://` staging is parked for MVP01+.
- Go module paths that differ from their repository paths, monorepo command
  selection, other install backends, configuration, and Windows are later work.

The full design record and acceptance criteria live in
[devdocs/PLAN-MVP00.md](devdocs/PLAN-MVP00.md).

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run
cargo check
git diff --check
```
