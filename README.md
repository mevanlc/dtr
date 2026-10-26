# dtr

**Do the Repo:** clone repositories, install their tools, and keep a kit of
installs you can repeat. Dtr accepts local paths, GitHub shorthand, and Git URLs,
then invokes Git, gh/glab, Cargo, Go, uv/pipx, or npm.

## Keep your tools in a kit

```sh
dtr kit add .                     # install this checkout and remember how
dtr kit add --tool cargo owner/tool -- --locked
dtr kit install -n                # preview what needs installing or rebuilding
dtr kit install                   # run the kit; skip matching recorded revisions
dtr kit list                      # show reusable install commands
dtr kit remove owner/tool         # stop tracking; keep binaries and checkout
```

A kit records repositories, installers, and native arguments in
`~/.config/dtr/kit.toml`. Add with `--no-install` to track without installing,
or use `dtr install --add` to record a successful one-off install.

Kit installs run concurrently (`-j N` sets the limit). Dtr skips entries whose
recorded Git revision and install request still match; dirty local checkouts
are rebuilt. Use `--force` to reinstall every entry, and `-v` to see skips and
commands. Local entries use the checkout as it stands; dtr does not pull it.

Short forms include `dtr k a`, `dtr k i`, `dtr k ls`, and `dtr k rm`.
Use `dtr kit edit` to edit the file. See the
[kit manual](doc/MANUAL.md#manage-an-installation-kit) for caching, alternate
files, and batch behavior.

## Install dtr

Download a binary from [GitHub Releases](https://github.com/mevanlc/dtr/releases)
and put it on your PATH, or install with Cargo:

```sh
cargo install dtr --locked
```

From a checkout, use `cargo install --path . --locked`. Once installed,
`dtr install .` can rebuild dtr from its own checkout, including on Windows.

Release targets cover Linux, macOS, and Windows on x64 and ARM64, plus Android
ARM64 and FreeBSD ARM64. Install the underlying tools you need: Git for cloning,
the relevant language installer for installing, and optionally `gh` or `glab`
for forge integration. Bare repository names require authenticated `gh` state.

## Install from a repository

```sh
dtr install                       # auto-detect the installer in this directory
dtr install owner/tool            # install from GitHub
dtr install --tool cargo ./tool -- --locked --features color
dtr install --tool uv ./python-tool
dtr install --tool go --main ./go-tool
```

Auto mode recognizes `Cargo.toml`, `go.mod`, Python manifests, and `package.json`.
Mixed or missing evidence requires an explicit `--tool`:
`cargo` (`rust`), `go`, `uv`, `pipx`, or `npm`. Python auto mode prefers uv, then
pipx. Native installer arguments follow `--`.

Install sources are repositories, not registry package names. See the
[install manual](doc/MANUAL.md#install-from-a-repository) for backend-specific
arguments, Go version queries, and command selection.

## Clone

```sh
dtr clone owner/repo               # GitHub shorthand
dtr clone --depth 1 -O owner/repo   # clone into owner--repo
dtr clone -D owner/repo            # clone into owner/repo
dtr clone https://gitlab.com/group/repo checkout-dir
cd "$(dtr clone -q owner/repo)"     # stdout contains the absolute checkout path
```

`clone` also accepts local paths, SSH URLs, and generic Git remotes. Use `./tool`
for a local path; `owner/tool` always means GitHub. GitHub and GitLab clones
prefer `gh` and `glab`, falling back to Git when the forge CLI is unavailable.
Native Git clone options pass through. See [clone details](doc/MANUAL.md#clone).

## Preview and output

```sh
dtr -n clone owner/repo
dtr install -n .
dtr kit install -nv                # include skipped entries and commands
```

`-n` / `--dry-run` previews operations; discovery may still access the network.
For cloning, put `-n` **before** `clone`: `dtr clone -n` means Git's
`--no-checkout`.

`-q` shows compact results, `-qq` only dtr warnings and failure diagnostics;
`-v` adds commands and skips, and `-vv` explains decisions. Counts cancel and
adjust the configured baseline. Command-result stdout and dry-run plans remain
available at every level.

## Configuration and completion

```sh
dtr config set github.auth.auto_switch personal,work
dtr config set verbosity -1
dtr config list
dtr config edit
dtr completion zsh > ~/.zfunc/_dtr
```

GitHub account selection matches explicit repository owners against your
allowlist and uses stored `gh` tokens for supported operations without changing
the active account. Configuration lives in `~/.config/dtr/config.toml`;
`DTR_CONFIG_DIR` overrides the directory for both config and kit files.

Completion supports Bash, Elvish, Fish, PowerShell, and Zsh. Create the output
directory and configure your shell to load it. Run `dtr <command> --help` for
options, or read the [manual](doc/MANUAL.md) for the full reference.

## Development

Run `just check` for the local validation gate. See [RELEASING.md](RELEASING.md)
for builds and publishing, and [devdocs/FIRST-MVP.md](devdocs/FIRST-MVP.md) for
project history and roadmap. Release tags supply the version; checkout builds
report `0.0.0`.

[MIT license](LICENSE).
