#!/usr/bin/env bash
# Release steps run by .github/workflows/release.yml, one sub-command per job: bump, bench (on
# each OS), commit. It never pushes; the workflow pushes after every job is green.
#
# usage: scripts/release.sh bump <patch|minor|major|X.Y.Z>
#        VT_BENCH_MACHINE="<runner, os, cores, cpu, rustc>" scripts/release.sh bench
#        scripts/release.sh commit
# bench builds the last five releases beside this one and runs them all in turns; a fixture may
# fall to VT_BENCH_THROUGHPUT_TOLERANCE (default 0.75) of the last release's MiB/s.
# A kind bumps the current workspace.package.version; an explicit X.Y.Z is used as given.
# Under 0.x, major gives 0.(x+1).0 and minor or patch give 0.x.(y+1); from 1.0, plain semver.
# Bash 3.2 compatible (macOS /bin/bash): no ${x,,}, no mapfile, no associative arrays.
set -euo pipefail

PLATFORMS="linux windows macos"
# How many earlier releases a bench job measures beside this one. Each costs the job a build
# and its share of every round.
HISTORY=5

usage() {
    cat >&2 <<'EOF'
usage: scripts/release.sh bump <patch|minor|major|X.Y.Z>
       VT_BENCH_MACHINE="<runner, os, cores, cpu, rustc>" scripts/release.sh bench
       scripts/release.sh commit
EOF
}

fail() {
    echo "error: $1" >&2
    exit 1
}

require_actions() {
    [ "${GITHUB_ACTIONS:-}" = true ] || fail "releases are cut by the Release workflow (.github/workflows/release.yml)"
}

workspace_version() {
    local version
    version=$(awk '/^\[/ { p = ($0 == "[workspace.package]") } p && /^version = / { gsub(/[^0-9.]/, ""); print; exit }' Cargo.toml)
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "cannot read workspace.package.version from Cargo.toml"
    echo "$version"
}

# A breaking commit since the last tag ('!' after the type, or a 'BREAKING CHANGE:' footer) must
# be released as a major step from version $1 to $2 (docs/agents/commits.md).
require_breaking_bump() {
    local last oma omi nma nmi
    last=$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null) || return 0
    git log "$last..HEAD" --format='%s%n%b' | grep -Eq '^[a-z]+(\([^)]*\))?!:|^BREAKING CHANGE:' || return 0
    IFS=. read -r oma omi _ <<<"$1"
    IFS=. read -r nma nmi _ <<<"$2"
    if [ "$nma" -gt "$oma" ]; then
        return 0
    fi
    if [ "$oma" -eq 0 ] && [ "$nma" -eq 0 ] && [ "$nmi" -gt "$omi" ]; then
        return 0
    fi
    fail "commits since $last carry '!' or 'BREAKING CHANGE:'; release with kind 'major', not v$2"
}

# Rewrites the version in the lines of file $1 that tell a reader which tag to depend on.
retag() {
    awk -v v="$2" '
        /tag = "v[0-9]+\.[0-9]+\.[0-9]+"/ { sub(/tag = "v[0-9]+\.[0-9]+\.[0-9]+"/, "tag = \"v" v "\"") }
        /^\*\*Status:\*\* v[0-9]/ { sub(/v[0-9]+\.[0-9]+\.[0-9]+/, "v" v) }
        { print }
    ' "$1" >.tmp/retag.new
    mv .tmp/retag.new "$1"
}

# HEAD must be the bump commit of version $1.
require_release_head() {
    [ "$(git log -1 --format=%s)" = "release: v$1" ] || fail "HEAD is not the bump commit 'release: v$1'"
}

cmd_bump() {
    local arg=$1 old ma mi pa v
    old=$(workspace_version)
    IFS=. read -r ma mi pa <<<"$old"
    case "$arg" in
        major) if [ "$ma" -eq 0 ]; then v="0.$((mi + 1)).0"; else v="$((ma + 1)).0.0"; fi ;;
        minor) if [ "$ma" -eq 0 ]; then v="0.$mi.$((pa + 1))"; else v="$ma.$((mi + 1)).0"; fi ;;
        patch) v="$ma.$mi.$((pa + 1))" ;;
        *) v="$arg" ;;
    esac
    echo "releasing v$old -> v$v"
    require_actions

    [ -z "$(git status --porcelain)" ] || fail "working tree is not clean; commit or stash first"
    [ "$(git symbolic-ref --short HEAD 2>/dev/null)" = main ] || fail "releases are cut from main"
    ! git rev-parse -q --verify "refs/tags/v$v" >/dev/null || fail "tag v$v already exists"
    require_breaking_bump "$old" "$v"
    local p
    for p in $PLATFORMS; do
        [ ! -e "perf/history/$p/v$v.json" ] || fail "perf/history/$p/v$v.json already exists"
        [ ! -e "perf/in-turns/$p/v$v.json" ] || fail "perf/in-turns/$p/v$v.json already exists"
    done

    mkdir -p .tmp
    for p in README.md docs/user/README.md docs/user/versioning.md; do
        retag "$p" "$v"
    done
    # Bump the version inside [workspace.package] only.
    awk -v v="$v" '
        /^\[/ { in_package = ($0 == "[workspace.package]") }
        in_package && /^version = / { print "version = \"" v "\""; next }
        { print }
    ' Cargo.toml >.tmp/Cargo.toml.new
    mv .tmp/Cargo.toml.new Cargo.toml
    grep -qF "version = \"$v\"" Cargo.toml || fail "version bump did not apply to Cargo.toml"

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace

    git add Cargo.toml Cargo.lock README.md docs/user/README.md docs/user/versioning.md
    git commit --allow-empty -m "release: v$v"
    echo "bumped v$v"
}

# Builds vt-bench of the last $HISTORY releases for platform $1 and prints one path per line,
# oldest first; prints nothing when no release is tagged yet. HEAD is the bump commit, so every
# tag it contains is an earlier release.
previous_benches() {
    local tags tag newest toolchain tree=.tmp/previous exe=""
    tags=$(git tag --list 'v[0-9]*' --merged HEAD --sort=v:refname | tail -n "$HISTORY")
    [ -n "$tags" ] || return 0
    newest=$(printf '%s\n' "$tags" | tail -n 1)
    [ "$1" != windows ] || exe=".exe"
    # Read here, outside the tags' tree: their rust-toolchain.toml must not pick another
    # compiler, or the builds would differ in more than the code.
    toolchain=$(rustup show active-toolchain | cut -d ' ' -f 1)
    git worktree add --detach "$tree/src" "$newest" >&2
    for tag in $tags; do
        git -C "$tree/src" checkout --quiet --detach "$tag" >&2
        # One target folder for every tag: what they share is built once.
        if (cd "$tree/src" && RUSTUP_TOOLCHAIN="$toolchain" cargo build -p oneterm-vt-bench --release --target-dir ../target) >&2; then
            cp "$tree/target/release/vt-bench$exe" "$tree/vt-bench-$tag$exe"
            echo "$tree/vt-bench-$tag$exe"
        elif [ "$tag" = "$newest" ]; then
            fail "$tag does not build with $toolchain: this release cannot be held against it"
        else
            echo "warning: $tag does not build with $toolchain; it is left out" | tee -a "$GITHUB_STEP_SUMMARY" >&2
        fi
    done
}

# Tier 2 of this build ($2) and of the last releases, every build run in turns in this job: the
# trip-wire (this build against the newest of them) and the figures the chart draws. A figure
# in MiB/s of a hosted runner cannot be held against one of another run: the same runner
# description measured an unchanged engine a factor of three apart.
in_turns() {
    local previous binary against=""
    previous=$(previous_benches "$1")
    for binary in $previous; do
        against="$against --against $binary"
    done
    if [ -z "$against" ]; then
        echo "no earlier release to measure beside this one: no trip-wire, no in-turns record"
        return 0
    fi
    mkdir -p "perf/in-turns/$1"
    # shellcheck disable=SC2086 # $against is a list of flags
    cargo run -p oneterm-vt-bench --release -- grid --check $against \
        --tolerance "${VT_BENCH_THROUGHPUT_TOLERANCE:-0.75}" \
        --machine "$VT_BENCH_MACHINE" --out "perf/in-turns/$1/v$2.json" | tee -a "$GITHUB_STEP_SUMMARY"
}

record() {
    mkdir -p "perf/history/$1"
    # With --json and --out, stdout carries the markdown report of the same measurement.
    cargo run -p oneterm-vt-bench --release -- all --mib 32 --json \
        --machine "$VT_BENCH_MACHINE" --out "perf/history/$1/v$2.json" | tee -a "$GITHUB_STEP_SUMMARY"
}

cmd_bench() {
    [ -n "${VT_BENCH_MACHINE:-}" ] || fail "VT_BENCH_MACHINE must name the machine (<runner, os, cores, cpu, rustc>)"
    require_actions
    local p v
    p=$(printf '%s' "${RUNNER_OS:-}" | tr '[:upper:]' '[:lower:]')
    case " $PLATFORMS " in
        *" $p "*) ;;
        *) fail "unknown platform '${RUNNER_OS:-}'; RUNNER_OS must be Linux, Windows or macOS" ;;
    esac
    v=$(workspace_version)
    require_release_head "$v"
    [ ! -e "perf/history/$p/v$v.json" ] || fail "perf/history/$p/v$v.json already exists"
    [ ! -e "perf/in-turns/$p/v$v.json" ] || fail "perf/in-turns/$p/v$v.json already exists"

    cargo test --workspace
    in_turns "$p" "$v"
    record "$p" "$v"
    echo "recorded perf/history/$p/v$v.json"
}

cmd_commit() {
    require_actions
    local v p
    v=$(workspace_version)
    require_release_head "$v"
    ! git rev-parse -q --verify "refs/tags/v$v" >/dev/null || fail "tag v$v already exists"
    for p in $PLATFORMS; do
        [ -e "perf/history/$p/v$v.json" ] || fail "perf/history/$p/v$v.json is missing"
    done
    # Only the first release has no earlier one to be measured beside, and so no chart.
    if git describe --tags --abbrev=0 --match 'v[0-9]*' HEAD >/dev/null 2>&1; then
        for p in $PLATFORMS; do
            [ -e "perf/in-turns/$p/v$v.json" ] || fail "perf/in-turns/$p/v$v.json is missing"
            cargo run -p oneterm-vt-bench --release -- chart --in-turns "perf/in-turns/$p/v$v.json" --out "perf/chart-$p.svg"
        done
    fi
    [ -z "$(git status --porcelain -- . ':(exclude)perf')" ] || fail "files outside perf/ changed; the release commit takes only perf/"

    git add perf
    git commit --amend --no-edit
    git tag -a "v$v" -m "oneterm-vt v$v"
    echo "tagged v$v"
}

case "${1:-}" in
    bump)
        { [ $# -eq 2 ] && [[ "$2" =~ ^(patch|minor|major|[0-9]+\.[0-9]+\.[0-9]+)$ ]]; } || { usage; exit 2; }
        ;;
    bench | commit)
        [ $# -eq 1 ] || { usage; exit 2; }
        ;;
    *)
        usage
        exit 2
        ;;
esac

cd "$(git rev-parse --show-toplevel)"
case "$1" in
    bump) cmd_bump "$2" ;;
    bench) cmd_bench ;;
    commit) cmd_commit ;;
esac
