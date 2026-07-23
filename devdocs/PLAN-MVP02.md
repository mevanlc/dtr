# dtr MVP02 plan

Status: implemented and validated (2026-07-23)

## Product increment

MVP02 returns to dtr's central repository-install surface by adding Rust/Cargo
installation:

```text
dtr install --rust <dtr-repospec>
dtr install --cargo <dtr-repospec>
```

Cargo already installs registry packages well. Dtr does not replace that
surface. It removes the source-shape distinction between Cargo's `--path` and
`--git` modes, applies the shared repospec grammar, and carries the GitHub
account auto-switch policy into authenticated Git installs.

`--rust` is the primary spelling and `--cargo` is its visible alias. The former
names the ecosystem consistently with future `--go`, `--uv`, and `--npm`
selectors; the latter remains immediately discoverable to Cargo users.

## MVP02 scope

MVP02 delivers:

- `dtr install --rust|--cargo <repospec>`.
- Local repository mapping to `cargo install --path`.
- Remote repository mapping to `cargo install --git`.
- Explicit pass-through of native Cargo install arguments after `--`.
- GitHub `github.auth.auto_switch` support for remote Cargo installs.
- Cargo-compatible conversion of SCP-like Git remotes to full SSH URLs.
- Exact, secret-free `--explain` output.
- macOS and Linux tests and validation.

MVP02 intentionally does not include:

- Registry crate installation. Continue to run `cargo install <crate>`.
- Repository inspection or automatic installer selection.
- Automatic package selection in a multi-package Git repository or workspace.
- Dtr-owned Cargo feature, target, profile, root, binary, or revision options;
  native Cargo options are forwarded after `--`.
- Rust installation from literal `scp://` or `sftp://` staging URLs.
- GitLab or generic-host account selection.
- Persisting credentials or modifying Cargo, Git, or GitHub CLI configuration.

## Usage as the functional requirements document

```text
dtr [--explain|-n] install|i <--rust|--cargo> <dtr-repospec>
dtr [--explain|-n] install|i <--rust|--cargo> <dtr-repospec> -- <cargo-install-arg>...
```

Examples:

```console
$ dtr --explain install --rust ./gittop
repospec: local repository ./gittop
backend:  cargo
command:  cargo install --path ./gittop

$ dtr --explain install --cargo hjr265/gittop -- --locked
repospec: GitHub repository hjr265/gittop
backend:  cargo
command:  cargo install --git https://github.com/hjr265/gittop.git --locked

$ dtr install --rust owner/workspace -- package-name --bin binary-name
# cargo install --git https://github.com/owner/workspace.git package-name --bin binary-name
```

Cargo arguments must follow `--`. This creates a stable boundary between dtr's
installer/repository interface and Cargo's native install interface. Dtr rejects
forwarded source selectors `--git`, `--path`, `--registry`, and `--index`, because
they would replace or conflict with the resolved repository source.

The Rust and Go selectors are mutually exclusive. `--no-latest` remains a Go
remote-install option and is rejected with `--rust` or `--cargo`. Cargo arguments
are likewise rejected for `--go` in MVP02.

## Repository mapping

### Local repository

```text
dtr install --rust ./tool
# cargo install --path ./tool
```

The original local `OsString` path is passed directly, including non-UTF-8 Unix
paths. Dtr does not require the path to exist during planning; Cargo owns the
native filesystem and manifest diagnostics during execution.

### Recognized forge repository

```text
dtr install --rust owner/tool
# cargo install --git https://github.com/owner/tool.git

dtr install --rust https://gitlab.com/group/tool
# cargo install --git https://gitlab.com/group/tool.git
```

Recognized GitHub and GitLab references become normalized HTTPS repository-root
URLs with a `.git` suffix. A bare repository name resolves the current GitHub
owner with `gh` and then uses the same normalized GitHub URL.

### Generic Git URL

An explicit `http://`, `https://`, `ssh://`, or `git://` URL is passed to
Cargo's `--git` mode unchanged when it is not a recognized forge URL.

Cargo documents that its built-in Git support does not accept SCP-like shorthand
such as `git@example.com:owner/tool.git`. Dtr converts that form without changing
its identity or path:

```text
git@example.com:owner/tool.git
ssh://git@example.com/~/owner/tool.git

git@example.com:/srv/git/tool.git
ssh://git@example.com/srv/git/tool.git
```

The `/~/` form preserves the SCP-like form's path relative to the remote user's
home directory. An SCP-like path that already begins `/` remains absolute.

Literal `scp://` and `sftp://` continue to produce the existing focused parked
feature error.

## Native Cargo arguments

Dtr does not duplicate Cargo's install option parser. Everything after the
separator is appended to the resolved command in the original `OsString` form:

```text
dtr install --rust owner/tool -- --locked --features color --bin tool
cargo install --git https://github.com/owner/tool.git --locked --features color --bin tool
```

This supports Cargo's package arguments and options such as `--branch`, `--tag`,
`--rev`, `--bin`, `--features`, `--locked`, `--root`, `--force`, and `--target`
without dtr maintaining a second copy of their arity rules.

Only the four source-replacing long options are rejected. Both separated and
equals forms are covered:

```text
--git URL       --git=URL
--path PATH     --path=PATH
--registry NAME --registry=NAME
--index URL     --index=URL
```

## GitHub account auto-switching

Given:

```toml
[github.auth]
auto_switch = ["mevanlc", "mike-clark-8192"]
```

an explicit matching owner applies to Rust installs as well as clones:

```console
$ dtr --explain install --rust mike-clark-8192/tool
repospec: GitHub repository mike-clark-8192/tool
backend:  cargo
auth:     auto-switch to mike-clark-8192 (process-scoped; active gh account unchanged)
command:  cargo install --git https://github.com/mike-clark-8192/tool.git
```

The implementation:

1. Obtains the selected stored token through `gh auth token --hostname
   github.com --user <account>` after removing inherited `GH_TOKEN` and
   `GITHUB_TOKEN` from that lookup.
2. Forces Cargo to use the Git executable with
   `CARGO_NET_GIT_FETCH_WITH_CLI=true`. Cargo documents this mode for Git
   authentication requirements that its built-in Git library does not support.
3. Extends Git's process-scoped `GIT_CONFIG_COUNT` / `GIT_CONFIG_KEY_<n>` /
   `GIT_CONFIG_VALUE_<n>` environment entries instead of overwriting existing
   entries.
4. Resets inherited `http.https://github.com/.extraHeader` values and adds one
   URL-scoped HTTP Basic authorization header containing the selected token.
5. Removes inherited `GH_TOKEN` and `GITHUB_TOKEN` from the Cargo child.

The token and derived authorization header are secret environment values. They
are never placed in argv, written to configuration or a temporary file, shown
by explain, included in an error, or given a generic debug representation. The
Git configuration is process-scoped and does not mutate global or repository Git
configuration.

As with every `cargo install`, the selected repository's build scripts and
procedural macros execute with the user's permissions. Auto-switching should be
used only for repositories the user is willing to build and execute.

If a matching allowlisted token cannot be retrieved, dtr fails before starting
Cargo. An unmatched owner retains normal Cargo authentication behavior.

## Explain behavior

The existing single-command `CommandPlan` remains sufficient. Auth material is
attached as secret environment, while the rendered plan contains only:

- Normalized repospec.
- `cargo` backend.
- Optional process-scoped account decision.
- Exact Cargo argv.

Explain may perform the read-only `gh auth token` lookup for a matched owner so
the plan is executable, but it never starts Cargo.

## Implementation structure

MVP02 extends:

```text
src/cli.rs          mutually exclusive Go/Rust selectors and Cargo argv boundary
src/repospec.rs     Cargo Git remote conversion
src/resolve.rs      Go and Cargo install planners
src/github_auth.rs  process-scoped Git HTTP authentication environment
tests/cli.rs        PATH-isolated Cargo and secret-handling behavior
```

No shell command string is constructed. Execution remains a direct
`std::process::Command` invocation.

## Testing strategy

Unit tests cover:

- Cargo remote conversion for every supported repospec family.
- SCP-like conversion with and without an explicit SSH user.
- Source-selector rejection, including equals forms.
- Existing `GIT_CONFIG_COUNT` parsing and extension.
- HTTP Basic header construction without displaying the result.
- CLI selector conflicts and the post-`--` argument boundary.

PATH-isolated integration tests cover:

- Exact local `cargo install --path` argv.
- Exact GitHub, GitLab, generic URL, SCP-like, and bare-name `--git` argv.
- `--cargo` as a visible alias of `--rust`.
- Native Cargo argument preservation.
- Go behavior remaining unchanged and rejecting Cargo-only arguments.
- Missing Cargo and child exit-status behavior.
- Matching GitHub auth selection, Git-CLI mode, inherited Git config extension,
  and absence of `GH_TOKEN` / `GITHUB_TOKEN` in the Cargo child.
- Unmatched and bare-name active-account behavior.
- Token lookup failure preventing Cargo execution.
- Explain showing the decision and exact argv without secret material.

Stable changes run:

```text
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo nextest run --locked
cargo check --locked
actionlint .github/workflows/ci.yml
git diff --check
```

The suite runs on macOS and Linux. Live validation is explain-only and must not
install a private repository or display credential material.

## Implementation sequence

1. [x] Add the Rust selector, Cargo alias, and native-argument boundary.
2. [x] Add Cargo-compatible repospec-to-source conversion.
3. [x] Plan local `--path` and remote `--git` commands.
4. [x] Reject forwarded source selectors that conflict with dtr resolution.
5. [x] Extend GitHub auto-switching with process-scoped Git HTTP auth.
6. [x] Add unit and PATH-isolated integration coverage.
7. [x] Update README and validate live explain behavior.
8. [x] Complete macOS/Linux gates, install, and milestone commit.

## MVP02 acceptance criteria

MVP02 is complete when:

- [x] `--rust` and `--cargo` resolve the same Cargo install operation.
- [x] Local and remote repository sources map to `--path` and `--git` exactly.
- [x] Supported Cargo args pass through after `--` without UTF-8 loss.
- [x] Forwarded source selectors cannot replace dtr's resolved source.
- [x] Go install behavior and its `--no-latest` contract remain intact.
- [x] GitHub owner matches select the configured account without shared-state
  mutation or secret output.
- [x] Existing process-scoped Git config is preserved when auth is added.
- [x] Explain and errors do not reveal token-derived values.
- [x] Cargo receives no inherited `GH_TOKEN` or `GITHUB_TOKEN` during a matched
  auto-switch operation.
- [x] No command is executed through a shell.
- [x] All validation gates pass on macOS and Linux.

## Validation record

Completed on 2026-07-23:

- macOS: `cargo fmt --check`, locked clippy with warnings denied, all 81 nextest
  tests, `cargo check --locked`, actionlint, and `git diff --check` passed.
- Linux: Rust 1.97 container validation passed formatting, locked clippy, all 81
  Cargo tests including doctests, and `cargo check --locked`.
- A synthetic local binary crate was installed through
  `dtr install --rust <path> -- --root <temporary-root>` and executed
  successfully.
- Live explain validation retrieved each allowlisted account independently for
  `mevanlc` and `mike-clark-8192`; the active `gh` account remained `mevanlc`,
  and no credential material appeared in output.

## External behavior references

- Cargo install source and selection behavior:
  <https://doc.rust-lang.org/cargo/commands/cargo-install.html>
- Cargo Git authentication and Git-CLI mode:
  <https://doc.rust-lang.org/cargo/appendix/git-authentication.html>
- Cargo `net.git-fetch-with-cli` configuration:
  <https://doc.rust-lang.org/cargo/reference/config.html#netgit-fetch-with-cli>
- Git process-scoped configuration and `http.extraHeader`:
  <https://git-scm.com/docs/git-config>
- GitHub CLI account token selection:
  <https://cli.github.com/manual/gh_auth_token>
