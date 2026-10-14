#!/bin/sh
set -eu
repo_root=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
# shellcheck source=scripts/cargo-target-cache.sh
. "$repo_root/scripts/cargo-target-cache.sh"
mkdir -p "$repo_root/target"
test_root=$(mktemp -d "$repo_root/target/cache-prune.XXXXXX")
trap 'rm -rf "$test_root"' 0
mkdir -p "$test_root/project/src" "$test_root/tmp" "$test_root/artifacts"
TMPDIR="$test_root/tmp"
export TMPDIR
printf '[package]\nname="cache-prune-fixture"\nversion="0.0.0"\nedition="2024"\n[workspace]\n' > "$test_root/project/Cargo.toml"
printf 'fn main() {}\n' > "$test_root/project/src/main.rs"
printf 'keep\n' > "$test_root/artifacts/saved-binary"
cache_dir="$test_root/target"
cargo check --quiet --offline --manifest-path "$test_root/project/Cargo.toml" --target-dir "$cache_dir"
cache_kib=$(du -sk "$cache_dir" | awk '{ print $1 }')

prune_cargo_target "$cache_dir" "$(((cache_kib + 1) * 1024))"
test -f "$cache_dir/CACHEDIR.TAG"
prune_cargo_target "$cache_dir" "$((cache_kib * 1024))"
test ! -e "$cache_dir"
test -f "$test_root/artifacts/saved-binary"
test -f "$test_root/project/src/main.rs"
prune_cargo_target "$cache_dir"

ln -s "$test_root/artifacts" "$cache_dir"
if prune_cargo_target "$cache_dir" 1; then
    echo 'Symlinked target cache was not rejected.' >&2
    exit 1
fi
test -f "$test_root/artifacts/saved-binary"
rm "$cache_dir"

mkdir "$cache_dir"
printf 'keep\n' > "$cache_dir/not-a-cargo-cache"
if prune_cargo_target "$cache_dir" 1; then
    echo 'Untagged target cache was not rejected.' >&2
    exit 1
fi
test -f "$cache_dir/not-a-cargo-cache"

if cargo_target_parent "$HOME/.."; then
    echo 'Parent-directory target was not rejected.' >&2
    exit 1
fi
if (HOME="$test_root/artifacts" cargo_target_parent "$cache_dir"); then
    echo 'Target outside HOME was not rejected.' >&2
    exit 1
fi

if [ "$(uname -s)" = FreeBSD ]; then
    lock_cargo_target "$cache_dir"
    if (lock_cargo_target "$cache_dir"); then
        echo 'Concurrent cache lock was not rejected.' >&2
        exit 1
    else
        test "$?" -eq 75
    fi
    exec 9>&-
fi
echo 'Cargo target cache checks passed.'
