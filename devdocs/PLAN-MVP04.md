# dtr MVP04 plan

Status: implemented and validated on macOS and Linux (2026-07-23)

Roadmap: [dtr first MVP](FIRST-MVP.md), phase 5 of 5

※ Historical note: this document records the interface and scope delivered by
MVP04. Its `--npm` selector spelling and statements that automatic installer
detection was excluded are intentionally preserved as phase history. The
current install interface and the completed automatic-selection increment are
documented in [PLAN-AUTO-INSTALL.md](PLAN-AUTO-INSTALL.md).

A marked statement or section contains interface or roadmap status that has
since been superseded.

## ※ Product increment

MVP04 adds repository-oriented JavaScript tool installation through npm and
closes the first MVP:

```text
dtr install --npm <dtr-repospec>
```

npm already installs registry package specifications well. Dtr does not wrap
that surface. It maps the shared repospec grammar to a local directory or Git
package source, installs the package globally, and applies the existing GitHub
account-selection policy to Git fetches.

## MVP04 scope

MVP04 delivers:

- ※ The explicit `--npm` installer selector.
- Global installation from local and Git repositories.
- Explicit native npm arguments after dtr's `--` separator.
- A single-source safety boundary for npm's multi-package install command.
- GitHub `github.auth.auto_switch` through process-scoped Git HTTP
  authentication.
- Exact, secret-free `--explain` output.
- Cross-backend help, errors, documentation, and regression coverage.
- Real local-package smoke tests, macOS/Linux gates, and a first-MVP audit.

※ MVP04 intentionally does not include npm registry packages, automatic installer
detection, monorepo workspace selection, literal `scp://` or `sftp://` staging,
non-GitHub account selection, release publishing, or Git tags.

## ※ Usage as the functional requirements document

```text
dtr [--explain|-n] install|i --npm <dtr-repospec>
dtr [--explain|-n] install|i --npm <dtr-repospec> -- <npm-install-option>...
```

Examples:

```console
$ dtr --explain install --npm ./my-tool
repospec: local repository ./my-tool
backend:  npm
command:  npm install --global -- ./my-tool

$ dtr --explain install --npm owner/tool -- --force
repospec: GitHub repository owner/tool
backend:  npm
command:  npm install --global --force -- git+https://github.com/owner/tool.git
```

The Go, Rust, uv, pipx, and npm selectors are mutually exclusive.
`--no-latest` remains Go-only and is rejected by every other backend.

## ※ Repository mapping

Local paths are passed directly. Remote repositories receive npm's Git package
source spelling after shared Git remote normalization:

```text
./tool                                  ./tool
owner/tool                              git+https://github.com/owner/tool.git
https://gitlab.com/group/tool           git+https://gitlab.com/group/tool.git
https://example.com/git/tool.git        git+https://example.com/git/tool.git
git://example.com/owner/tool.git        git://example.com/owner/tool.git
git@example.com:owner/tool.git          git+ssh://git@example.com/~/owner/tool.git
git@example.com:/srv/tool.git           git+ssh://git@example.com/srv/tool.git
```

A bare name resolves the current GitHub owner with `gh`. Literal `scp://` and
`sftp://` retain the parked-feature error. npm owns package metadata, `bin`
selection, dependency installation, lifecycle scripts, and their diagnostics.
Users must trust repositories they install.

## ※ Native installer arguments

Dtr invokes:

```text
npm install --global <forwarded-options> -- <resolved-source>
```

HTTP(S) and SSH sources require npm's `git+` prefix. npm's native `git://`
transport remains unprefixed because `git+git://` is not a supported package
protocol.

Current npm accepts multiple positional package specifications. To preserve
dtr's repository-only contract, every forwarded token must begin with `-`, and
dtr places its one resolved source after npm's own `--`. Option values therefore
use attached forms such as `--prefix=/opt/npm`. A forwarded standalone `--` is
rejected. `-g`, `--global`, `--no-global`, and their value forms are rejected
because dtr owns the global-install invariant; `--global-style` remains a
distinct valid npm option.

## ※ GitHub account auto-switching

An explicit GitHub owner matching `github.auth.auto_switch` applies to npm. Dtr:

1. Retrieves the selected token through `gh auth token --hostname github.com
   --user <account>` without inherited GitHub token variables.
2. Extends process-scoped Git configuration with an empty reset followed by a
   URL-scoped `http.https://github.com/.extraHeader` Basic authorization header,
   preserving existing process Git configuration.
3. Removes inherited `GH_TOKEN` and `GITHUB_TOKEN` from the npm child.

Dtr supplies a full `git+https` repository source, which npm fetches through
Git. It does not run `gh auth switch` or mutate Git, GitHub CLI, or npm
configuration. The token-derived header is secret environment, never argv, and
never appears in explain, errors, configuration, temporary files, or generic
debug output.

Unmatched owners and bare names retain native active-account behavior. A
matched owner whose token cannot be retrieved fails before npm starts.

## ※ Explain and implementation

The command plan renders the normalized repospec, `npm` backend, optional account
decision, and exact child argv. Secret environment remains omitted. Explain may
perform the read-only token lookup but never starts npm.

MVP04 extends:

```text
src/cli.rs          npm selector and shared native-argument boundary
src/repospec.rs     npm package source conversion
src/resolve.rs      npm install planner and argument safety boundary
src/github_auth.rs  process-scoped npm Git authentication environment
tests/cli.rs        PATH-isolated argv, auth, help, and safety behavior
```

No shell command string is constructed for the resolved install operation.

## ※ Testing and validation

Tests cover selector exclusivity; source mapping for every repospec family;
non-UTF-8 paths and arguments; npm single-source and global-mode restrictions;
exact argv; missing npm and exit statuses; all prior backend regressions;
matched, unmatched, bare, failed, and inherited GitHub auth; and secret-free
explain output.

Stable changes run:

```text
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo nextest run --locked
cargo check --locked
actionlint .github/workflows/ci.yml
git diff --check
```

The suite runs on macOS and Linux. A real local npm CLI package is installed and
executed with an isolated global prefix. Live account validation is explain-only
and displays no secrets.

## ※ Implementation sequence

1. [x] Add the npm selector and npm package-source mapping.
2. [x] Plan an exact global npm invocation with a single-source boundary.
3. [x] Add process-scoped GitHub HTTP authentication.
4. [x] Add unit and PATH-isolated integration coverage.
5. [x] Complete cross-backend help, error, and documentation review.
6. [x] Validate a real local npm install and live explain behavior.
7. [x] Complete macOS/Linux gates, install, and milestone commit.
8. [x] Audit and close the first-MVP roadmap.

## ※ MVP04 acceptance criteria

- [x] `--npm` conflicts with every other installer selector.
- [x] Local paths and remote npm Git sources map exactly.
- [x] Native arguments remain lossless and cannot add a second npm package.
- [x] Native arguments cannot disable dtr's global-install invariant.
- [x] Existing Go, Cargo, uv, and pipx behavior remains intact.
- [x] Owner matches select the configured GitHub account without shared mutation
  or secret output.
- [x] Existing process-scoped Git configuration is preserved.
- [x] Help and README examples cover every supported first-MVP operation.
- [x] No resolved operation is executed through a shell.
- [x] All validation gates pass on macOS and Linux.

## ※ Validation record

Completed on 2026-07-23:

- macOS passed formatting, locked warnings-as-errors clippy, all 107 nextest
  tests, locked check, actionlint, and diff validation.
- Linux passed formatting, locked warnings-as-errors clippy, all 107 Cargo tests
  plus doctests, and locked check under Rust 1.97.1.
- A synthetic JavaScript CLI package was installed through dtr with npm 11.17.0
  into an isolated global prefix, and the resulting executable ran successfully.
- Live explain validation selected both `mevanlc` and `mike-clark-8192` for npm.
  The active `gh` account remained `mevanlc`, and no credential material appeared
  in output.
- PATH-isolated integration tests cover exact local and every remote source form,
  option safety, selector conflicts, missing and failing npm, matched/unmatched/
  bare auth, inherited Git config, fail-closed token lookup, and exact explain.
- npm 11.17.0's package-spec parser confirmed that generic HTTPS and SSH Git
  sources require the `git+` prefix, while `git://` must remain unprefixed; a
  live parse check confirmed npm's `--` boundary treats the following local path
  as the sole package source.
- The release build was installed as `$HOME/.cargo/bin/dtr` and reports
  version 0.1.0.

## External behavior references

- npm install: <https://docs.npmjs.com/cli/v11/commands/npm-install/>
- npm package specifications:
  <https://docs.npmjs.com/cli/v11/using-npm/package-spec/>
- Git process-scoped configuration: <https://git-scm.com/docs/git-config>
