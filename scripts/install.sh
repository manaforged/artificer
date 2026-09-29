#!/bin/sh
set -eu

usage() {
    printf '%s\n' \
        'Usage: ./scripts/install.sh [--help|--uninstall]' \
        '' \
        'Builds Artificer, then runs `artificer install`: the launchers, the' \
        'recorded Cargo path, ~/.artificer/env, and one PATH line in your shell' \
        'profiles. It does not edit editor configuration.'
}

if [ "$#" -gt 1 ]; then
    usage >&2
    exit 2
fi

case "${1:-}" in
    --help|-h)
        usage
        exit 0
        ;;
    --uninstall)
        : "${HOME:?HOME is required}"
        cargo_home=${CARGO_HOME:-"$HOME/.cargo"}
        bin="$cargo_home/bin/artificer"
        if [ -x "$bin" ]; then
            "$bin" uninstall
        else
            rm -f "$bin" "$HOME/.artificer/bin/cargo" \
                "$HOME/.artificer/real-cargo" "$HOME/.artificer/env" \
                "$HOME/.artificer/store"
            printf 'Removed Artificer launchers. The cache remains.\n'
        fi
        exit 0
        ;;
    '') ;;
    *)
        usage >&2
        exit 2
        ;;
esac

: "${HOME:?HOME is required}"
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cargo_home=${CARGO_HOME:-"$HOME/.cargo"}
real=${ARTIFICER_REAL_CARGO:-}
if [ -z "$real" ] && [ -r "$HOME/.artificer/real-cargo" ]; then
    IFS= read -r real <"$HOME/.artificer/real-cargo" || real=
fi
if [ -z "$real" ] || [ ! -x "$real" ]; then
    real=$(command -v cargo || true)
fi
if [ "$real" = "$HOME/.artificer/bin/cargo" ]; then
    real="$cargo_home/bin/cargo"
fi
case "$real" in
    */toolchains/*)
        if [ -x "$cargo_home/bin/cargo" ]; then
            real="$cargo_home/bin/cargo"
        else
            printf '%s\n' \
                'Cargo resolved to a Rustup toolchain binary, not its proxy.' \
                'Set ARTIFICER_REAL_CARGO to a system Cargo executable or Rustup proxy.' >&2
            exit 1
        fi
        ;;
esac
if [ -z "$real" ] || [ ! -x "$real" ]; then
    printf 'Cargo executable not found. Set ARTIFICER_REAL_CARGO.\n' >&2
    exit 1
fi

printf 'Building Artificer with %s\n' "$real"
"$real" build --release --locked --target-dir "$root/target" \
    --manifest-path "$root/Cargo.toml"

ARTIFICER_REAL_CARGO="$real" "$root/target/release/artificer" install
