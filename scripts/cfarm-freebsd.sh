#!/usr/bin/env bash
# Use the home-local CFARM toolchain through SSH; never install a remote runner.
set -euo pipefail
umask 077

mode=${1:-}
if [[ $# != 1 || ! $mode =~ ^(probe|build)$ ]]; then
    echo 'Usage: bash scripts/cfarm-freebsd.sh <probe|build>' >&2
    exit 1
fi
cd "$(dirname "$0")/.."
if [[ $mode == build ]]; then
    release_version=$(uv run scripts/release.py version "${RELEASE_VERSION:?Set RELEASE_VERSION}")
fi

: "${CFARM_FBSD15_HOST:?Set CFARM_FBSD15_HOST}"
: "${CFARM_FBSD15_KEY:?Set CFARM_FBSD15_KEY}"
: "${RUNNER_TEMP:?Set RUNNER_TEMP}"

destination=${CFARM_FBSD15_HOST//$'\r'/}
ssh_user=${CFARM_FBSD15_SSH_USER:-mevanlc}
ssh_port=${CFARM_FBSD15_SSH_PORT:-22}
if [[ ! $destination =~ ^([A-Za-z0-9._-]+@)?[A-Za-z0-9][A-Za-z0-9.-]*$ ]]; then
    echo 'CFARM_FBSD15_HOST must be a hostname or user@hostname.' >&2
    exit 1
fi
if [[ $destination != *@* ]]; then
    if [[ ! $ssh_user =~ ^[A-Za-z0-9_][A-Za-z0-9._-]*$ ]]; then
        echo 'Invalid SSH login.' >&2
        exit 1
    fi
    destination="$ssh_user@$destination"
fi
if [[ ! $ssh_port =~ ^[0-9]{1,5}$ ]] || ((10#$ssh_port < 1 || 10#$ssh_port > 65535)); then
    echo 'SSH port must be between 1 and 65535.' >&2
    exit 1
fi

probe_dir=$(mktemp -d "$RUNNER_TEMP/dtr-freebsd-probe.XXXXXX")
remote_dir=''
ssh_options=(
    -F /dev/null
    -i "$probe_dir/key"
    -p "$ssh_port"
    -o BatchMode=yes
    -o IdentitiesOnly=yes
    -o IdentityAgent=none
    -o PasswordAuthentication=no
    -o KbdInteractiveAuthentication=no
    -o ConnectTimeout=20
    -o ConnectionAttempts=1
    -o ServerAliveInterval=10
    -o ServerAliveCountMax=3
    -o ControlMaster=auto
    -o ControlPersist=60
    -o "ControlPath=$probe_dir/control"
    -o StrictHostKeyChecking=yes
    -o HostKeyAlias=cfarm-fbsd15
    -o HostKeyAlgorithms=ssh-ed25519
    -o "UserKnownHostsFile=$PWD/scripts/cfarm-known_hosts"
)
cleanup() {
    local status=$?
    if [[ -n $remote_dir ]]; then
        if ! ssh "${ssh_options[@]}" "$destination" sh -s "$remote_dir" <<'REMOTE_CLEANUP'
set -eu
rm -rf -- "$1"
REMOTE_CLEANUP
        then
            echo 'Failed to remove the remote probe directory.' >&2
            status=1
        fi
    fi
    ssh "${ssh_options[@]}" -O exit "$destination" >/dev/null 2>&1 || true
    rm -rf -- "$probe_dir"
    exit "$status"
}
trap cleanup EXIT

printf '%s\n' "$CFARM_FBSD15_KEY" | tr -d '\r' > "$probe_dir/key"
unset CFARM_FBSD15_KEY
ssh-keygen -y -P '' -f "$probe_dir/key" > /dev/null

echo 'Checking SSH authentication and remote build tools...'
ssh "${ssh_options[@]}" "$destination" 'sh -s' <<'REMOTE'
set -eu
PATH="$HOME/.cargo/bin:/usr/local64/bin:/usr/local/bin:$PATH"
export PATH
system=$(uname -s)
printf 'Operating system: %s\n' "$system"
if [ "$system" != FreeBSD ]; then
    echo 'Expected a FreeBSD host.' >&2
    exit 1
fi
printf 'FreeBSD kernel/userland versions:\n'
freebsd-version -ku
printf 'Architecture: %s\n' "$(uname -m)"
command -v rsync
rsync --version | head -n 1
for tool in git cc rustc cargo; do
    if command -v "$tool" >/dev/null 2>&1; then
        "$tool" --version | head -n 1
    else
        printf '%s: not installed or not on PATH\n' "$tool"
    fi
done
if command -v make >/dev/null 2>&1; then
    printf 'make version: %s\n' "$(make -V MAKE_VERSION)"
else
    echo 'make: not installed or not on PATH'
fi
REMOTE

remote_dir=$(ssh "${ssh_options[@]}" "$destination" sh -s "$mode" <<'REMOTE_TEMP'
set -eu
mkdir -p "$HOME/.local/share/dtr-ci/tmp"
mktemp -d "$HOME/.local/share/dtr-ci/tmp/$1.XXXXXXXX"
REMOTE_TEMP
)
if [[ ! $remote_dir =~ ^/[A-Za-z0-9/_.-]+/dtr-ci/tmp/(probe|build)\.[A-Za-z0-9]+$ ]]; then
    remote_dir=''
    echo 'Unexpected remote temporary-directory path.' >&2
    exit 1
fi

# Quote each SSH argument for rsync's remote-shell parser.
printf -v rsync_shell '%q ' ssh "${ssh_options[@]}"
# Expand PATH on the FreeBSD host rather than on the runner.
# shellcheck disable=SC2016
rsync_path='PATH="/usr/local64/bin:/usr/local/bin:$PATH" rsync'
if [[ $mode == build ]]; then
    target=aarch64-unknown-freebsd
    echo 'Uploading the stamped source snapshot...'
    rsync --archive --rsh="$rsync_shell" --rsync-path="$rsync_path" \
        --exclude=/.git/ --exclude=/target/ --exclude=/dist/ \
        --exclude=/.cfarm-artifacts/ ./ "$destination:$remote_dir/source/"
    ssh "${ssh_options[@]}" "$destination" sh -s "$remote_dir" <<'REMOTE_BUILD'
set -eu
if [ "$(uname -m)" != arm64 ]; then
    echo 'Expected an ARM64 FreeBSD host.' >&2
    exit 1
fi
cd "$1/source"
sh scripts/build-freebsd.sh
REMOTE_BUILD
    mkdir -p "target/$target/release"
    rsync --archive --rsh="$rsync_shell" --rsync-path="$rsync_path" \
        "$destination:$remote_dir/source/.cfarm-artifacts/dtr" "target/$target/release/dtr"
    binary="target/$target/release/dtr"
    readelf --file-header "$binary" | grep -E 'Machine:.*AArch64'
    readelf --program-headers "$binary" | grep -F '/libexec/ld-elf.so.1]'
    uv run scripts/release.py archive "$target"
    archive="dist/dtr-v$release_version-$target.tar.gz"
    rsync --archive --rsh="$rsync_shell" --rsync-path="$rsync_path" \
        "$archive" "$destination:$remote_dir/download.tar.gz"
    echo 'Smoke-testing the packaged release download on CFARM...'
    ssh "${ssh_options[@]}" "$destination" sh -s "$remote_dir" "$release_version" <<'REMOTE_SMOKE'
set -eu
mkdir "$1/smoke"
tar -xzf "$1/download.tar.gz" -C "$1/smoke"
binary="$1/smoke/dtr-v$2-aarch64-unknown-freebsd/dtr"
actual=$("$binary" --version)
if [ "$actual" != "dtr $2" ]; then
    printf 'Unexpected packaged version: %s\n' "$actual" >&2
    exit 1
fi
"$binary" --help >/dev/null
REMOTE_SMOKE
    echo 'FreeBSD ARM64 build and packaged smoke test passed.'
    if [[ -n ${GITHUB_STEP_SUMMARY:-} ]]; then
        printf 'FreeBSD ARM64 built and tested on CFARM; packaged download passed version/help checks.\n' \
            >> "$GITHUB_STEP_SUMMARY"
    fi
    exit 0
fi

printf 'dtr FreeBSD connectivity probe %s\n' "${GITHUB_RUN_ID:-local}" > "$probe_dir/sent.txt"
echo 'Checking rsync upload and download...'
rsync --archive --rsh="$rsync_shell" --rsync-path="$rsync_path" \
    "$probe_dir/sent.txt" "$destination:$remote_dir/probe.txt"
rsync --archive --rsh="$rsync_shell" --rsync-path="$rsync_path" \
    "$destination:$remote_dir/probe.txt" "$probe_dir/received.txt"
cmp "$probe_dir/sent.txt" "$probe_dir/received.txt"
echo 'SSH and rsync round trip passed.'
if [[ -n ${GITHUB_STEP_SUMMARY:-} ]]; then
    printf 'SSH authentication and rsync upload/download to FreeBSD passed. The probe file is removed on exit.\n' \
        >> "$GITHUB_STEP_SUMMARY"
fi
