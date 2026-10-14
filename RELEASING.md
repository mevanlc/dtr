# Releases

This repository releases one Rust package containing the `dtr` command, following
the [tuisv release setup](https://github.com/mevanlc/tuisv/blob/main/RELEASING.md).
It publishes seven binary archives and one crate from the same commit.
The Git tag supplies the version; committed package versions stay at `0.0.0`.

## Normal releases

1. Merge changes into `main` and let CI pass.
2. Tag the intended commit and push that tag:

   ```sh
   git tag -a v0.1.0 -m 'Release v0.1.0'
   git push origin v0.1.0
   ```

Use a new version for each publication. Stable tags have the form `vX.Y.Z`.
Prereleases such as `v0.2.0-rc.1` are supported; build-metadata suffixes are not.
Prereleases are published to crates.io and marked as prereleases on GitHub,
without becoming the stable latest release.

The release workflow validates and stamps the version, runs all checks, builds
and packages every target, stages a complete GitHub draft, publishes the crate,
then publishes the draft. Immutable releases lock the tag and assets and include
GitHub's release attestation. Generated source-code archives from GitHub reflect
the committed `0.0.0` manifest; the attached `.crate` contains the stamped version.

## First publication and credentials

Enable immutable releases in the repository's release settings. Create the
`crates-io` GitHub Actions environment without required reviewers, so tag releases
publish automatically.

For a new crate, log into crates.io, verify the account email, and create a
short-lived API token that can create/publish this crate. Add it as the
`CARGO_REGISTRY_TOKEN` secret in the `crates-io` environment using GitHub's secret
UI or `gh secret set CARGO_REGISTRY_TOKEN --env crates-io`. Enter it through the
hidden prompt; never put the value in source, command arguments, logs, or chat.

After the first publication:

1. In the crate's crates.io settings, add a GitHub trusted publisher:
   owner `mevanlc`, repository `dtr`, workflow `release.yml`, environment `crates-io`.
2. Run Release manually on `main` with mode `verify-oidc`. It obtains a temporary
   token and automatically revokes it at job completion; it cannot publish.
3. Delete the bootstrap `CARGO_REGISTRY_TOKEN` secret. Revoke its crates.io token
   only if it was dedicated to this bootstrap; keep a shared token valid for
   other projects that still use it.

The workflow uses the bootstrap secret when present and otherwise uses OIDC.
An already-published matching crate needs neither credential mechanism to be
uploaded again. Tokens are supplied only to the publishing step; builds run
without publishing credentials.

## Validation without publishing

Run Release manually with mode `build` and a version such as `0.1.0` or
`0.2.0-rc.1`. This runs the complete build and package checks and stores Actions
artifacts. Manual runs cannot create releases or publish crates.

Pull requests and pushes to `main` run quality checks, all seven platform builds,
and crate packaging/install checks. They use version `0.0.0` and do not publish.

Local checks:

```sh
just check
cargo nextest run --locked --run-ignored only -E 'test(real_cargo_repeated_self_install)'
cargo publish --dry-run --locked
```

Use a disposable checkout for stamping tests:
`uv run scripts/release.py stamp 0.1.0` deliberately edits the local manifest and
the package's lockfile entry. It verifies that no dependency versions change.
The helper assumes it is run from the repository root.

The six desktop/server targets execute nextest and doctests, exercise repeated
real Cargo self-installation in an isolated install root, and smoke-test the
extracted download with `--help` and `--version`. The internal `dtr-trampoline`
example is compiled and exercised by tests but is not installed or distributed
as a separate command. Linux binaries are checked for static
linking. Android uses NDK 27.3.13750724 with API 24 and is checked for the correct
ELF architecture, interpreter, and system-library dependencies. Android execution
is not covered by hosted CI. macOS uses deployment target 11.0. FreeBSD builds
are deferred while the shared infrastructure is developed.

The manual `FreeBSD connectivity` workflow probes the CFARM build host from a
GitHub-hosted Ubuntu runner using the repository secrets `CFARM_FBSD15_KEY`
(an unencrypted SSH private key) and `CFARM_FBSD15_HOST` (hostname or
`user@hostname`). Dispatch inputs select the SSH login and port, defaulting to
`mevanlc` and `22`. It reports the OS, architecture, and available build tools,
then verifies an rsync upload/download in a temporary directory and removes it.
Remote probe files live under `$HOME/.local/share/dtr-ci/tmp`.
No Actions runner or toolchain is installed on the remote machine. The probe
uses first-use host-key trust in a temporary known-hosts file; pin the host key
before using this connection for release builds.

For a direct SSH build, rsync a source snapshot into
`$HOME/.local/share/dtr-ci/source` on the FreeBSD host, then run
`sh scripts/build-freebsd.sh` there. Install the pinned Rust toolchain with
Clippy and rustfmt under `$HOME/.cargo` first; ARM64 uses the official standalone
installer because its rustup bootstrap download is unavailable. The script
sets PATH explicitly, checks the compiler version, and keeps Cargo caches,
build output, temporary files, and the resulting binary under `$HOME`. It uses
two build jobs on the shared host and runs formatting, Clippy, tests (including
real isolated self-installation), doctests, and a release-binary smoke check.
Nextest is used when available, with a serial Cargo test harness as a fallback
to avoid executable-fixture `ETXTBSY` races on CheriBSD.
On CheriBSD ARM64, the script creates a home-local `cc` wrapper selecting the
conventional ARM64 ABI, libraries, linker mode, and `/libexec/ld-elf64.so.1`
loader required by the official Rust toolchain. The system compiler is unchanged.

Crate verification compiles the packaged source and installs from its unpacked
contents. Before publishing, the upload job recreates the crate and checks its
SHA-256 against the verified CI artifact. Its `cargo publish --no-verify` skips
only the compilation already performed by CI.

## Downloads and verification

Binary archives use `dtr-vVERSION-TARGET.tar.gz`, or `.zip` for Windows, and
contain a matching top-level directory with the executable, README, release
guide, and MIT license. Each release also contains:

- `dtr-VERSION.crate`: the exact crate published to crates.io.
- `release-manifest.json`: source commit, Rust toolchain, target matrix, and hashes.
- `SHA256SUMS`: hashes for all the above assets, including the manifest.

Download all assets into an empty directory and verify them:

```sh
gh release download v0.1.0 --repo mevanlc/dtr
sha256sum --check SHA256SUMS
```

On macOS, use `shasum -a 256 --check SHA256SUMS`. On Windows, use
`Get-FileHash -Algorithm SHA256` to compare a file with its entry in `SHA256SUMS`.
GitHub's `gh release verify` and `gh release verify-asset` additionally verify
the immutable release attestation.

## Recovery

Start by rerunning failed jobs in the original Actions run. Successful build
artifacts remain available for 30 days; the assembled release bundle is retained
for 90 days. Retrying a failed publication reuses that run's successful artifacts.

- Before publication, a failure leaves the draft and any successfully uploaded
  assets in place.
- Existing assets must have matching hashes. Only missing assets are uploaded.
  Differing or unexpected assets stop the workflow; nothing is clobbered.
- If crate upload succeeded but a later operation failed, the retry checks the
  registry checksum and skips the upload only when the bytes match.
- If Cargo times out after uploading, the helper polls the registry for up to
  five minutes before failing; retrying reconciles an upload that appeared later.
- A completed matching release is a successful no-op on retry.

Do not move tags, delete releases, or replace published versions to fix an
application bug. Publish a new version. If a full rebuild differs from an
existing asset, resume with the original run's artifacts; pinned tools and
normalized archive metadata do not make hosted runner images immutable.

## Adapting this example

Copy the two workflows, the release helper and tests, the target matrix,
`rust-toolchain.toml`, `.gitattributes`, and the relevant packaging/license metadata. This is a
copyable example, not a centrally hosted workflow dependency.

- Update the Cargo package/binary names, repository URL, description, license,
  and documentation. Replace the executable and crate paths containing `dtr`
  in CI; the helper derives the package and binary names from the manifest.
- Adjust `scripts/release-targets.json` and platform setup for the new project.
  Its entries also define the expected complete release asset set.
- Keep test fixtures and other compile-time files in the crate's `include` list.
- Configure immutable releases, the `crates-io` environment, and the new crate's
  trusted-publisher identity. Each new crate needs its own initial token publish.
- Update Rust in `rust-toolchain.toml`. Both workflows read that pin. Update uv,
  Python, nextest, actionlint, and Android NDK pins deliberately and rerun the
  full manual build. Dependabot proposes updates to pinned GitHub Actions monthly.

The helper supports one package with explicitly listed executables. A workspace
needs explicit dependency-order publishing and is outside this example.
