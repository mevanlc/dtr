# dtr MVP01 plan

Status: implemented and validated on macOS and Linux (2026-07-23)

Roadmap: [dtr first MVP](FIRST-MVP.md), phase 2 of 5

※ Historical note: this document records the interface and scope delivered by
MVP01. Statements that repository inspection or installer backends were parked
describe that phase, not current behavior. The current install interface and
the completed automatic-selection increment are documented in
[PLAN-AUTO-INSTALL.md](PLAN-AUTO-INSTALL.md).

A marked statement or section contains interface or roadmap status that has
since been superseded.

## Product increment

MVP01 teaches `dtr clone` to choose the right authenticated GitHub account when
the repository owner provides an unambiguous identity signal. It adds the first
small, inspectable configuration surface needed to control that behavior.

The motivating case is:

```text
dtr clone mike-clark-8192/foo
dtr clone mevanlc/bar
```

When both accounts are authenticated in GitHub CLI, dtr should use the token for
the owner named in each repository reference without running `gh auth switch`
or changing GitHub CLI's globally active account.

“Auto-switch” is intentionally the user-facing term. It connects the feature to
the familiar `gh auth switch` workflow while the implementation remains
process-scoped and race-free.

## MVP01 scope

MVP01 delivers:

- A configuration file and `config set`, `config get`, and `config unset`.
- The `github.auth.auto_switch` account allowlist.
- Owner-to-account matching for GitHub shorthand and GitHub URL clones.
  ※ Recognized GitHub SSH forms gained the same matching behavior after MVP01.
- Process-scoped GitHub token selection for an eligible clone.
- Auth decisions in `--explain` output without displaying credentials.
- Deterministic, PATH-isolated tests for configuration and account selection.

MVP01 intentionally does not include:

- Calling `gh auth switch` or modifying GitHub CLI configuration.
- Persistently binding the resulting checkout to an account for later
  `git fetch` or `git push` operations.
- Selecting among SSH keys. Auto-switched clones use HTTPS because the selected
  credential is a GitHub token.
- GitHub Enterprise Server, `ghe.com`, or configurable forge hosts.
- GitLab account switching.
- Automatic account selection for bare repository names, which contain no owner
  signal and continue to use GitHub CLI's active account.
- ※ Other items parked by MVP00, including additional installer backends,
  automatic installer detection, and `scp://` / `sftp://` staging.

## Usage as the functional requirements document

```text
dtr config set github.auth.auto_switch <account>[,<account>...]
dtr config get github.auth.auto_switch
dtr config unset github.auth.auto_switch
```

For example:

```console
$ dtr config set github.auth.auto_switch mevanlc,mike-clark-8192

$ dtr config get github.auth.auto_switch
mevanlc,mike-clark-8192

$ dtr config unset github.auth.auto_switch
```

`set` accepts one non-empty comma-separated account list. Surrounding
whitespace is ignored, account matching is ASCII case-insensitive, and duplicate
accounts are removed while preserving the first spelling and order. Empty list
members and account names containing characters outside letters, digits, `-`,
`_`, and `.` are rejected.

`get` prints the normalized comma-separated value. It returns a focused error
when the key is not set. `unset` is idempotent.

MVP01 recognizes exactly one public configuration key. Unknown keys and unknown
fields in the configuration file are errors rather than silently ignored auth
configuration.

## Configuration storage

The configuration file is TOML. Its default location is:

```text
${XDG_CONFIG_HOME}/dtr/config.toml  when XDG_CONFIG_HOME is set
${HOME}/.config/dtr/config.toml     otherwise
```

`DTR_CONFIG_DIR` overrides the containing directory. This is useful for isolated
automation and tests; the filename remains `config.toml`.

※ The default-location rule above is superseded: dtr now ignores
`XDG_CONFIG_HOME` and always uses `<user-home>/.config/dtr/config.toml` when
`DTR_CONFIG_DIR` is unset.

※ Clone and install treat an undiscoverable configuration location as an empty
configuration. Explicitly invalid overrides and errors reading or parsing a
discovered configuration file still fail the operation.

The setting is represented as a TOML array even though the CLI's compact value
uses commas:

```toml
[github.auth]
auto_switch = ["mevanlc", "mike-clark-8192"]
```

Writes create the parent directory and replace the file atomically. The file
stores account names only. Tokens obtained from GitHub CLI are never written to
dtr configuration.

## `github.auth.auto_switch` contract

The value is an allowlist of accounts dtr may automatically switch to. For a
GitHub repository with an explicit owner:

1. Compare the owner with configured accounts case-insensitively.
2. If there is no match, retain normal MVP00 behavior and let `gh` use its active
   account.
3. If there is a match and the `gh` backend is available, request that account's
   stored token with:

   ```text
   gh auth token --hostname github.com --user <configured-account>
   ```

4. Run the resolved `gh repo clone` child with that token in `GH_TOKEN` only for
   the child process. `GH_TOKEN` and `GITHUB_TOKEN` inherited by the token lookup
   subprocess are removed so they cannot replace the requested stored account.
5. Use an explicit HTTPS GitHub repository URL for the clone so token selection
   is not undermined by a globally configured SSH transport.

GitHub CLI documents that `gh auth token --user` reads a particular stored
account token and that `GH_TOKEN` takes precedence over stored credentials for
the process. This is the race-free primitive; dtr never copies or rewrites
GitHub CLI configuration.

If the owner matches an allowlisted account but its token cannot be obtained,
dtr fails before cloning. Falling back to the active account in this case would
silently violate an explicit auth policy. Diagnostics name the account but never
include token output.

If `gh` is unavailable, the existing public-URL `git clone` fallback remains in
place and no auto-switch is attempted. Git itself reports whether that fallback
has sufficient credentials.

### Resolution examples

Given:

```toml
[github.auth]
auto_switch = ["mevanlc", "mike-clark-8192"]
```

| Repository reference | Result |
|---|---|
| `mike-clark-8192/foo` | process-scoped switch to `mike-clark-8192` |
| `mevanlc/bar` | process-scoped switch to `mevanlc` |
| `cli/cli` | no allowlist match; use active account |
| `https://github.com/mevanlc/bar` | process-scoped switch to `mevanlc` |
| `foo` | no explicit owner; use active account |
| `https://gitlab.com/mevanlc/bar` | non-GitHub backend; setting does not apply |
| `./mevanlc/bar` | local repository; setting does not apply |

## Explain behavior and secret handling

For a matched account, `--explain` includes the decision and its scope:

```text
repospec: GitHub repository mike-clark-8192/foo
backend:  gh
auth:     auto-switch to mike-clark-8192 (process-scoped; active gh account unchanged)
target:   foo
command:  gh repo clone https://github.com/mike-clark-8192/foo.git
```

Explain mode may perform the read-only token lookup because the executable
command plan is otherwise incomplete. It must never print the token, render it
as an environment assignment, place it in an error, or run the final clone.

The typed command plan carries environment values separately from rendered
argv. Secret environment values are applied only during execution and have no
generic debug or display representation.

## CLI and implementation structure

MVP01 adds:

```text
src/config.rs       typed schema, location, parsing, atomic persistence, commands
src/github_auth.rs  owner matching and read-only account-token retrieval
```

Configuration parsing remains separate from repository parsing. GitHub auth
selection occurs only after the repospec has been recognized as GitHub and the
`gh` backend has been selected.

The external clone remains represented once as a typed `CommandPlan`; explain
and execution consume the same plan. The plan gains:

- An optional human-readable auth decision.
- Process environment assignments whose values are never included in command
  rendering.

No shell command strings are constructed.

## Testing strategy

Unit tests cover:

- Account-list parsing, whitespace, case-insensitive deduplication, and invalid
  members.
- TOML parsing, unknown-field rejection, and stable serialization.
- Configuration-path precedence.
- Case-insensitive owner matching.

PATH-isolated integration tests cover:

- `config set`, `get`, and idempotent `unset` using `DTR_CONFIG_DIR`.
- A matching owner receiving the requested account token in the clone child.
- A matching GitHub URL receiving the same behavior.
  ※ Current coverage also includes recognized GitHub SSH forms.
- An unmatched owner and a bare repository retaining active-account behavior.
- A matching configured account whose token lookup fails, with no clone started.
- Parent `GH_TOKEN` / `GITHUB_TOKEN` values not affecting stored-account lookup.
- `--explain` showing the auto-switch decision while hiding the token and not
  starting the clone.
- Missing `gh` retaining the existing `git clone` fallback.

Stable changes run:

```text
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo nextest run --locked
cargo check --locked
actionlint .github/workflows/ci.yml
git diff --check
```

The suite runs on macOS and Linux. Live tests may verify both locally configured
GitHub accounts in explain mode, but must not display tokens or clone private
repositories as part of the default gate.

## Implementation sequence

1. [x] Add the typed TOML schema and deterministic configuration-path handling.
2. [x] Add `config set`, `config get`, and `config unset`.
3. [x] Add secret process-environment support to `CommandPlan`.
4. [x] Match explicit GitHub owners against `github.auth.auto_switch`.
5. [x] Retrieve the selected account token without inherited token overrides.
6. [x] Force HTTPS and apply `GH_TOKEN` only to the resolved clone child.
7. [x] Explain the process-scoped decision without exposing the credential.
8. [x] Complete unit, integration, macOS, and Linux validation.
9. [x] Update README from the verified behavior and mark MVP01 complete.

## MVP01 acceptance criteria

MVP01 is complete when:

- [x] The documented config commands round-trip the allowlist atomically.
- [x] Unknown or malformed auth configuration fails with a focused diagnostic.
- [x] Both configured personal accounts can be selected from the owner in an
  explicit GitHub repository reference without changing the active `gh` account.
- [x] Unmatched owners and bare names preserve MVP00 behavior.
- [x] An allowlisted match that lacks a retrievable token fails closed.
- [x] Auto-switched clone transport is HTTPS; SSH-key selection is not implied.
- [x] Explain and all diagnostics remain free of token values.
- [x] The final operation is still executed without a shell.
- [x] All validation gates pass on macOS and Linux.

### ※ Validation record

- macOS: formatting, Clippy with warnings denied, `cargo check`, and all 61 tests
  through nextest pass with the locked dependency graph.
- Linux: the Rust 1.97 slim container passes formatting, Clippy with warnings
  denied, all 61 tests through Cargo's built-in harness (nextest is absent in
  the clean container), and `cargo check`.
- PATH-isolated integration tests verify configuration round trips, exact
  account-token selection, HTTPS forcing, fail-closed lookup, inherited-token
  isolation, fallback behavior, and token-free explain output.
- Live macOS explain checks successfully select both locally authenticated
  GitHub accounts. The active account is `mevanlc` both before and after the
  checks, confirming that no shared `gh` auth state changes.

## External behavior references

- GitHub CLI account token selection: <https://cli.github.com/manual/gh_auth_token>
- GitHub CLI environment precedence: <https://cli.github.com/manual/gh_help_environment>
- GitHub CLI active-account mutation: <https://cli.github.com/manual/gh_auth_switch>
- GitHub CLI clone transport selection: <https://cli.github.com/manual/gh_repo_clone>
