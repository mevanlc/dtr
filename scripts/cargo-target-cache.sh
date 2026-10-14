# shellcheck shell=sh
# Source this file, then hold lock_cargo_target for the whole build/prune cycle.
# The caller keeps descriptor 9 open; its lock file lives outside the cache.

cargo_target_parent() (
    cache_dir=${1%/}
    case "$cache_dir" in
        /*) ;;
        *) echo 'Cargo target directory must be absolute.' >&2; return 1 ;;
    esac
    case "$(basename "$cache_dir")" in
        .|..) echo 'Cargo target directory must name a child directory.' >&2; return 1 ;;
    esac
    if [ -L "$cache_dir" ]; then
        echo 'Refusing to prune a symlinked Cargo target directory.' >&2
        return 1
    fi
    cache_parent=$(CDPATH='' cd "$(dirname "$cache_dir")" && pwd -P) || return
    cache_home=$(CDPATH='' cd "$HOME" && pwd -P) || return
    case "$cache_parent" in
        "$cache_home"|"$cache_home"/*) ;;
        *) echo 'Cargo target directory must be inside HOME.' >&2; return 1 ;;
    esac
    printf '%s\n' "$cache_parent"
)

lock_cargo_target() {
    cargo_cache_parent=$(cargo_target_parent "$1") || return
    cargo_cache_lock="$cargo_cache_parent/.$(basename "$1").build.lock"
    exec 9>"$cargo_cache_lock"
    if ! lockf -s -t 0 9; then
        printf 'Cargo target cache is in use: %s\n' "$1" >&2
        return 75
    fi
}

prune_cargo_target() (
    cache_dir=${1%/}
    cache_limit=${2:-3000000000}
    case "$cache_limit" in
        ''|*[!0-9]*|0*) echo 'Cache limit must be a positive decimal byte count.' >&2; return 1 ;;
    esac
    if [ "${#cache_limit}" -gt 18 ]; then
        echo 'Cache limit is too large.' >&2
        return 1
    fi
    cargo_target_parent "$cache_dir" >/dev/null || return
    if [ ! -e "$cache_dir" ]; then return 0; fi
    cache_usage=$(du -sk "$cache_dir") || return
    cache_kib=${cache_usage%%[[:space:]]*}
    cache_bytes=$((cache_kib * 1024))
    if [ "$cache_bytes" -lt "$cache_limit" ]; then return 0; fi
    if [ ! -f "$cache_dir/CACHEDIR.TAG" ] ||
        ! grep -qx 'Signature: 8a477f597d28d172789f06886806bc55' "$cache_dir/CACHEDIR.TAG"; then
        echo 'Refusing to prune a directory without a Cargo cache tag.' >&2
        return 1
    fi
    printf 'Pruning Cargo target cache: %s bytes (limit %s bytes).\n' "$cache_bytes" "$cache_limit"
    cargo clean --target-dir "$cache_dir"
)
