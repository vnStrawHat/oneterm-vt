#!/usr/bin/env bash
# Public-API snapshot: check or refresh the two committed files under crates/vt/.
#
# usage: scripts/public-api.sh <check|update>
#   check   regenerate the host's file and diff it against the committed one (CI)
#   update  regenerate both files, the non-host one by cross target, and print the diff stat
set -euo pipefail

# All pins live here; change them together with docs/specs/0014-release/public-api.md.
NIGHTLY=nightly-2026-05-25
CARGO_PUBLIC_API=0.52.0
UNIX_TARGET=x86_64-unknown-linux-gnu
WINDOWS_TARGET=x86_64-pc-windows-msvc

usage() {
    echo "usage: scripts/public-api.sh <check|update>" >&2
}

if [ $# -ne 1 ] || ! [[ "$1" =~ ^(check|update)$ ]]; then
    usage
    exit 2
fi
MODE="$1"

cd "$(git rev-parse --show-toplevel)"
mkdir -p .tmp

# cargo install puts the tool in $CARGO_HOME/bin, which a fresh shell may not have on PATH.
PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
        HOST_OS=windows
        HOST_TARGET="$WINDOWS_TARGET"
        OTHER_OS=unix
        OTHER_TARGET="$UNIX_TARGET"
        ;;
    *)
        HOST_OS=unix
        HOST_TARGET="$UNIX_TARGET"
        OTHER_OS=windows
        OTHER_TARGET="$WINDOWS_TARGET"
        ;;
esac

TOOL_VERSION="$(cargo public-api --version 2>/dev/null || true)"
if [[ "$TOOL_VERSION" != *"$CARGO_PUBLIC_API"* ]]; then
    cargo install cargo-public-api --version "$CARGO_PUBLIC_API" --locked
fi

# ensure_toolchain <target>...: install the pinned nightly with those standard libraries.
# Skipped when the nightly is already installed, so a second run needs no network.
ensure_toolchain() {
    # `rustup toolchain list` prints the host triple after the name, so match the prefix.
    if ! rustup toolchain list | grep -q "^$NIGHTLY-"; then
        local args=() target
        for target in "$@"; do
            args+=(--target "$target")
        done
        rustup toolchain install "$NIGHTLY" --profile minimal "${args[@]}"
    fi
}

# generate <os> <target>: write .tmp/public-api.<os>.txt for that target.
generate() {
    cargo +"$NIGHTLY" public-api -p oneterm-vt --all-features --simplified --color never \
        --target "$2" >".tmp/public-api.$1.txt"
}

if [ "$MODE" = check ]; then
    ensure_toolchain "$HOST_TARGET"
    generate "$HOST_OS" "$HOST_TARGET"
    if ! diff -u "crates/vt/public-api.$HOST_OS.txt" ".tmp/public-api.$HOST_OS.txt"; then
        echo "::error::public API changed; run scripts/public-api.sh update, commit both files, pick the bump in docs/user/versioning.md"
        exit 1
    fi
    exit 0
fi

ensure_toolchain "$HOST_TARGET" "$OTHER_TARGET"
generate "$HOST_OS" "$HOST_TARGET"
generate "$OTHER_OS" "$OTHER_TARGET"
mv ".tmp/public-api.$HOST_OS.txt" ".tmp/public-api.$OTHER_OS.txt" crates/vt/
git diff --stat -- crates/vt/public-api.*.txt
