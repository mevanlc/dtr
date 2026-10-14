#!/bin/sh
# Run from a source snapshot under the remote account's home directory.
set -eu

if [ "$(uname -s)" != FreeBSD ]; then
    echo 'This build script requires FreeBSD.' >&2
    exit 1
fi
cd "$(CDPATH='' cd "$(dirname "$0")/.." && pwd)"
remote_home=$(CDPATH='' cd "$HOME" && pwd -P)
case "$(pwd -P)" in
    "$remote_home"/*) ;;
    *) echo 'The source checkout must be under HOME.' >&2; exit 1 ;;
esac

build_root="$HOME/.local/share/dtr-ci"
PATH="$HOME/.cargo/bin:$PATH"
CARGO_HOME="$HOME/.cargo"
CARGO_TARGET_DIR="$build_root/target"
CARGO_BUILD_JOBS=2
TMPDIR="$build_root/tmp"
XDG_CACHE_HOME="$build_root/cache"
export PATH CARGO_HOME CARGO_TARGET_DIR CARGO_BUILD_JOBS TMPDIR XDG_CACHE_HOME
mkdir -p "$CARGO_TARGET_DIR" "$TMPDIR" "$XDG_CACHE_HOME" "$build_root/artifacts"

# CheriBSD's default cc uses the capability ABI. Rust's FreeBSD ARM64
# toolchain needs the conventional ABI, libraries, linker mode, and loader.
# A PATH wrapper also covers tests that invoke rustc directly.
if [ "$(uname -m)" = arm64 ] && /usr/bin/cc -dM -E -x c /dev/null | grep -q '__CHERI_PURE_CAPABILITY__'; then
    mkdir -p "$build_root/linker"
    cat > "$build_root/linker/cc" <<'LINKER'
#!/bin/sh
exec /usr/bin/cc -mabi=aapcs -Wl,-m,aarch64elf -Wl,--dynamic-linker,/libexec/ld-elf64.so.1 "$@"
LINKER
    chmod 700 "$build_root/linker/cc"
    PATH="$build_root/linker:$PATH"
    export PATH
    echo 'Using the conventional ARM64 ABI on CheriBSD.'
fi

expected_rust=$(awk -F '"' '/^channel = / { print $2 }' rust-toolchain.toml)
actual_rust=$(rustc --version | awk '{ print $2 }')
if [ "$actual_rust" != "$expected_rust" ]; then
    printf 'Expected Rust %s, found %s.\n' "$expected_rust" "$actual_rust" >&2
    exit 1
fi
rustc --version
cargo --version
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
if command -v cargo-nextest >/dev/null 2>&1; then
    cargo nextest run --locked
    cargo nextest run --locked --run-ignored only -E 'test(real_cargo_repeated_self_install)'
else
    echo 'Nextest is unavailable; using the Cargo test harness.'
    cargo test --locked --all-targets
    cargo test --locked --test trampoline real_cargo_repeated_self_install_keeps_only_dtr_installed -- --ignored --exact
fi
cargo test --locked --doc
cargo build --locked --release --bin dtr --example dtr-trampoline
"$CARGO_TARGET_DIR/release/dtr" --version
"$CARGO_TARGET_DIR/release/dtr" --help >/dev/null
cp "$CARGO_TARGET_DIR/release/dtr" "$build_root/artifacts/dtr"
printf 'FreeBSD validation passed; binary: %s/artifacts/dtr\n' "$build_root"
