# dtr MVP03 plan

Status: implemented and validated on macOS and Linux (2026-07-23)

Roadmap: [dtr first MVP](FIRST-MVP.md), phase 4 of 5

※ Historical note: this document records the interface and scope delivered by
MVP03. Its `--uv` / `--pipx` selector spellings and statements that Python
backend selection or repository inspection were excluded are intentionally
preserved as phase history. The current install interface is documented in
[PLAN-AUTO-INSTALL.md](PLAN-AUTO-INSTALL.md).

A marked statement or section contains interface or roadmap status that has
since been superseded.

## ※ Product increment

MVP03 adds repository-oriented Python tool installation through uv and pipx:

```text
dtr install --uv <dtr-repospec>
dtr install --pipx <dtr-repospec>
```

Both tools already install PyPI package specifications well. Dtr does not wrap
that registry surface. It maps the shared repospec grammar to the local path or
`git+<URL>` package source each tool expects and applies the existing GitHub
account-selection policy to Git fetches.

## MVP03 scope

MVP03 delivers:

- ※ Explicit `--uv` and `--pipx` installer selectors.
- Local repository installation through `uv tool install` and `pipx install`.
- Remote repository conversion to Python VCS requirement URLs.
- Explicit native installer arguments after dtr's `--` separator.
- A single-source safety boundary for pipx's multi-package install command.
- GitHub `github.auth.auto_switch` through process-scoped Git HTTP
  authentication.
- Exact, secret-free `--explain` output.
- PATH-isolated coverage, real local-package smoke tests, and macOS/Linux gates.

※ MVP03 intentionally does not include PyPI package installation, automatic
uv-versus-pipx selection, repository inspection, monorepo subdirectory
selection, literal `scp://` or `sftp://` staging, or non-GitHub account
selection. Dtr does not install pipx when the user selects it but it is absent.

## ※ Usage as the functional requirements document

```text
dtr [--explain|-n] install|i --uv <dtr-repospec>
dtr [--explain|-n] install|i --uv <dtr-repospec> -- <uv-tool-install-arg>...
dtr [--explain|-n] install|i --pipx <dtr-repospec>
dtr [--explain|-n] install|i --pipx <dtr-repospec> -- <pipx-install-option>...
```

Examples:

```console
$ dtr --explain install --uv ./my-tool
repospec: local repository ./my-tool
backend:  uv
command:  uv tool install ./my-tool

$ dtr --explain install --uv owner/tool -- --python 3.14
repospec: GitHub repository owner/tool
backend:  uv
command:  uv tool install git+https://github.com/owner/tool.git --python 3.14

$ dtr --explain install --pipx owner/tool -- --python=3.14 --force
repospec: GitHub repository owner/tool
backend:  pipx
command:  pipx install --python=3.14 --force -- git+https://github.com/owner/tool.git
```

The Go, Rust, uv, and pipx selectors are mutually exclusive. `--no-latest`
remains Go-only and is rejected by every other backend.

## ※ Repository mapping

Local paths are passed directly:

```text
dtr install --uv ./tool
uv tool install ./tool

dtr install --pipx ./tool
pipx install -- ./tool
```

Local paths and forwarded arguments remain `OsString` values, including
non-UTF-8 Unix paths. The selected backend owns packaging metadata and build
diagnostics.

Remote repositories receive Python packaging's `git+` VCS prefix after the
shared Git remote normalization:

```text
owner/tool                              git+https://github.com/owner/tool.git
https://gitlab.com/group/tool           git+https://gitlab.com/group/tool.git
https://example.com/git/tool.git        git+https://example.com/git/tool.git
git@example.com:owner/tool.git          git+ssh://git@example.com/~/owner/tool.git
git@example.com:/srv/tool.git           git+ssh://git@example.com/srv/tool.git
```

A bare name resolves the current GitHub owner with `gh`. Literal `scp://` and
`sftp://` retain the parked-feature error.

## ※ Native installer arguments

Dtr accepts native arguments only after its separator. Go continues to reject
them; Cargo behavior remains unchanged. Uv receives its source first and native
arguments afterward, preserving their order and values.

Current pipx accepts multiple positional package specifications. Dtr must never
turn its native-option escape hatch into registry installation. It places the
one resolved source after pipx's own `--` and requires every forwarded pipx token
to begin with `-`:

```text
dtr install --pipx owner/tool -- --python=3.14 --force
pipx install --python=3.14 --force -- git+https://github.com/owner/tool.git
```

Pipx option values must use attached spelling such as `--python=3.14`. A
separate value token is rejected because it is indistinguishable from a second
package without duplicating pipx's evolving option parser. A forwarded
standalone `--` is rejected. Pipx's `--lock` is rejected because it supplies an
alternate installation source that conflicts with the repospec.

## ※ GitHub account auto-switching

An explicit GitHub owner matching `github.auth.auto_switch` applies to both
Python backends. Dtr:

1. Retrieves the selected token through `gh auth token --hostname github.com
   --user <account>` without inherited GitHub token variables.
2. Extends process-scoped Git configuration with an empty reset followed by a
   URL-scoped `http.https://github.com/.extraHeader` Basic authorization header,
   preserving existing process Git configuration.
3. Removes inherited `GH_TOKEN` and `GITHUB_TOKEN` from the installer child.
4. Sets `UV_NO_GITHUB_FAST_PATH=true` so direct uv and pipx's possible uv
   backend fetch through Git instead of first trying an unauthenticated GitHub
   API shortcut.

Current uv source dispatches repository fetches through the Git CLI, so the same
process-scoped Git HTTP configuration works for direct uv, pipx's uv backend,
and pipx's pip backend. Dtr does not run `gh auth switch` or mutate Git, GitHub
CLI, uv, or pipx configuration. The token-derived header is secret environment,
never argv, and never appears in explain, errors, configuration, temporary
files, or generic debug output.

Unmatched owners and bare names retain native active-account behavior. A
matched owner whose token cannot be retrieved fails before the installer starts.

Python build backends and legacy setup code execute with the user's permissions,
as they do for direct uv or pipx installation. Users must trust repositories
they install.

## ※ Explain and implementation

The command plan renders the normalized repospec, backend, optional account
decision, and exact child argv. Secret environment remains omitted. Explain may
perform the read-only token lookup but never starts uv or pipx.

MVP03 extends:

```text
src/cli.rs          uv/pipx selectors and shared native-argument boundary
src/repospec.rs     Python VCS source conversion
src/resolve.rs      uv and pipx install planners
src/github_auth.rs  process-scoped Git HTTP authentication environment
tests/cli.rs        PATH-isolated argv, auth, and safety behavior
```

No shell command string is constructed for the resolved install operation.

## ※ Testing and validation

Tests cover selector exclusivity; source mapping for every repospec family;
non-UTF-8 values; pipx single-source restrictions; exact uv/pipx argv; missing
tools and exit statuses; Go/Cargo regression behavior; matched, unmatched, bare,
and failed GitHub auth; inherited Git configuration; and secret-free explain.

Stable changes run:

```text
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo nextest run --locked
cargo check --locked
actionlint .github/workflows/ci.yml
git diff --check
```

The suite runs on macOS and Linux. Real local-package smoke tests use isolated uv
and pipx homes. Live account validation is explain-only and displays no secrets.

## ※ Implementation sequence

1. [x] Add uv/pipx selectors and generalize the argument boundary.
2. [x] Add Python local/VCS source conversion.
3. [x] Plan exact uv and single-source pipx invocations.
4. [x] Add process-scoped GitHub HTTP authentication.
5. [x] Add unit and PATH-isolated integration coverage.
6. [x] Update README and first-MVP progress.
7. [x] Validate real local installs and live explain behavior.
8. [x] Complete macOS/Linux gates, install, and milestone commit.

## ※ MVP03 acceptance criteria

- [x] `--uv` and `--pipx` conflict with every other installer selector.
- [x] Local paths and remote `git+<URL>` sources map exactly.
- [x] Native arguments remain lossless and cannot add a second pipx package.
- [x] Pipx alternate-source arguments cannot replace the repospec.
- [x] Existing Go and Cargo behavior remains intact.
- [x] Owner matches select the configured GitHub account without shared mutation
  or secret output.
- [x] Existing process-scoped Git configuration is preserved.
- [x] No resolved install operation is executed through a shell.
- [x] All validation gates pass on macOS and Linux.

## ※ Validation record

Completed on 2026-07-23:

- macOS passed formatting, locked warnings-as-errors clippy, all 94 nextest
  tests, locked check, actionlint, and diff validation.
- Linux passed formatting, locked warnings-as-errors clippy, all 94 Cargo tests
  including doctests, and locked check under Rust 1.97.
- A uv-created synthetic CLI package was installed and executed through both
  `dtr install --uv` and `dtr install --pipx`, using isolated tool homes and bin
  directories.
- Live explain validation selected both `mevanlc` and `mike-clark-8192` for both
  Python backends. The active `gh` account remained `mevanlc`, and no credential
  material appeared in output.
- Current uv source at `e4e2f69` confirms its repository fetch path dispatches
  through the Git CLI; its GitHub fast path honors `UV_NO_GITHUB_FAST_PATH`.

## External behavior references

- Uv tool sources: <https://docs.astral.sh/uv/guides/tools/>
- Uv Git authentication:
  <https://docs.astral.sh/uv/concepts/authentication/git/>
- Pipx install CLI: <https://pipx.pypa.io/stable/reference/cli.html>
- Git process-scoped configuration: <https://git-scm.com/docs/git-config>
