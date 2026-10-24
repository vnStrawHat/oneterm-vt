#!/usr/bin/env bash
# Release helper: bump the version, run the gate and the benchmark, chart, commit, tag.
# Run by the owner on the benchmark machine. It commits and tags; it never pushes.
#
# usage: VT_BENCH_MACHINE="<cpu, cores, os>" scripts/release.sh <patch|minor|major|X.Y.Z>
# A kind bumps the current workspace.package.version; an explicit X.Y.Z is used as given.
# Under 0.x, major gives 0.(x+1).0 and minor or patch give 0.x.(y+1); from 1.0, plain semver.
set -euo pipefail

usage() {
    echo "usage: VT_BENCH_MACHINE=\"<cpu, cores, os>\" scripts/release.sh <patch|minor|major|X.Y.Z>" >&2
}

if [ $# -ne 1 ] || ! [[ "$1" =~ ^(patch|minor|major|[0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
    usage
    exit 2
fi
ARG="$1"

if [ -z "${VT_BENCH_MACHINE:-}" ]; then
    echo "error: VT_BENCH_MACHINE must describe the machine (CPU, cores, OS build)" >&2
    exit 1
fi

fail() {
    echo "error: $1" >&2
    exit 1
}

cd "$(git rev-parse --show-toplevel)"

# Resolve the target version; a kind bumps the current workspace.package.version.
OLD=$(awk '/^\[/ { p = ($0 == "[workspace.package]") } p && /^version = / { gsub(/[^0-9.]/, ""); print; exit }' Cargo.toml)
[[ "$OLD" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "cannot read workspace.package.version from Cargo.toml"
IFS=. read -r MA MI PA <<<"$OLD"
case "$ARG" in
    major) if [ "$MA" -eq 0 ]; then V="0.$((MI + 1)).0"; else V="$((MA + 1)).0.0"; fi ;;
    minor) if [ "$MA" -eq 0 ]; then V="0.$MI.$((PA + 1))"; else V="$MA.$((MI + 1)).0"; fi ;;
    patch) V="$MA.$MI.$((PA + 1))" ;;
    *) V="$ARG" ;;
esac
echo "releasing v$OLD -> v$V"

[ -z "$(git status --porcelain)" ] || fail "working tree is not clean; commit or stash first"
[ "$(git symbolic-ref --short HEAD 2>/dev/null)" = main ] || fail "releases are cut from main"
! git rev-parse -q --verify "refs/tags/v$V" >/dev/null || fail "tag v$V already exists"
[ ! -e "perf/history/v$V.json" ] || fail "perf/history/v$V.json already exists"

# The tree is modified from here on; leave it for the owner to inspect.
trap 'echo "release aborted; undo with: git checkout -- . && rm -f \"perf/history/v$V.json\"" >&2' ERR

mkdir -p .tmp

# Bump the version inside [workspace.package] only.
awk -v v="$V" '
    /^\[/ { in_package = ($0 == "[workspace.package]") }
    in_package && /^version = / { print "version = \"" v "\""; next }
    { print }
' Cargo.toml >.tmp/Cargo.toml.new
mv .tmp/Cargo.toml.new Cargo.toml
grep -qF "version = \"$V\"" Cargo.toml || fail "version bump did not apply to Cargo.toml"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace

mkdir -p perf/history
cargo run -p oneterm-vt-bench --release -- all --mib 32 --json \
    --machine "$VT_BENCH_MACHINE" --out "perf/history/v$V.json"
cargo run -p oneterm-vt-bench --release -- chart --history perf/history --out perf/chart.svg


git add Cargo.toml Cargo.lock "perf/history/v$V.json" perf/chart.svg
git commit -m "release: v$V"
# The tree is clean again; a tag failure is undone by dropping the release commit.
trap 'echo "release aborted after commit; undo with: git reset --hard HEAD~1" >&2' ERR
git tag -a "v$V" -m "oneterm-vt v$V"

echo "next: git push origin main \"v$V\""
