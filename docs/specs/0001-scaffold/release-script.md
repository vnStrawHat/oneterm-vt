> Note: the script now also accepts `patch|minor|major` in place of `<version>` and runs only inside
> the Release workflow (docs/specs/0014-release/workflow.md); the rest below is history.

# `scripts/release.sh <version>`

Bash, `set -euo pipefail`, LF endings, executable bit set (`git update-index --chmod=+x`).
Runs in Git Bash (Windows), Linux and macOS: no `sed -i`, no GNU-only flags; edits go through a
temp file in `.tmp/` then `mv`. Must pass `shellcheck` (CI runs it).

Run by the owner on the benchmark machine. It commits and tags; **it never pushes**.

## Inputs

| Input | Rule |
|---|---|
| `$1` | required; must match `^[0-9]+\.[0-9]+\.[0-9]+$`; otherwise usage on stderr, exit 2 |
| `VT_BENCH_MACHINE` env | required, non-empty: CPU, core count, OS build, e.g. `i7-12700 12C/20T, Windows 11 26200`. A perf record without a machine is meaningless. |

## Steps (stop at the first failure)

| # | Step | Command / check |
|---|---|---|
| 1 | Go to repo root | `cd "$(git rev-parse --show-toplevel)"` |
| 2 | Clean tree | `[ -z "$(git status --porcelain)" ]` else "commit or stash first" |
| 3 | On `main` | `[ "$(git symbolic-ref --short HEAD)" = main ]` |
| 4 | Tag is new | `! git rev-parse -q --verify "refs/tags/v$V"` |
| 5 | Record is new | `[ ! -e "perf/history/v$V.json" ]` |
| 6 | Changelog ready | `grep -q '^## \[Unreleased\]' CHANGELOG.md` |
| 7 | Bump version | inside `[workspace.package]` of root `Cargo.toml`: `version = "…"` becomes `version = "$V"` (awk/sed range to `.tmp/`, `mv` back); then `grep -q "^version = \"$V\"" Cargo.toml` |
| 8 | Quality gate | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace` (also refreshes `Cargo.lock`) |
| 9 | Benchmark | `mkdir -p perf/history` then `cargo run -p oneterm-vt-bench --release -- all --mib 32 --json --machine "$VT_BENCH_MACHINE" --out "perf/history/v$V.json"` |
| 10 | Chart | `cargo run -p oneterm-vt-bench --release -- chart --history perf/history --out perf/chart.svg` |
| 11 | Changelog | replace the first `## [Unreleased]` line with `## [Unreleased]`, blank line, `## [$V] - $(date -u +%Y-%m-%d)` |
| 12 | Commit | `git add Cargo.toml Cargo.lock CHANGELOG.md "perf/history/v$V.json" perf/chart.svg` then `git commit -m "release v$V"` |
| 13 | Tag | `git tag -a "v$V" -m "oneterm-vt v$V"` |
| 14 | Hint | print `next: git push origin main "v$V"` |

`--mib 32` keeps a full run at a few minutes; every record uses the same value so releases compare.

On failure after step 7 the tree is dirty. A `trap … ERR` prints:
`release aborted; undo with: git checkout -- . && rm -f "perf/history/v$V.json"`.
The script does not undo by itself: the owner may want to inspect the failure.

## Phase 0 acceptance for the script

Until the bench lands (phase 1+), step 9 fails with "not implemented", which is the intended
behaviour: no release without a perf record. Verify by hand now:

| Run | Expected |
|---|---|
| `scripts/release.sh` | usage, exit 2 |
| `scripts/release.sh 0.1` | usage, exit 2 |
| `VT_BENCH_MACHINE= scripts/release.sh 0.1.0` | error naming `VT_BENCH_MACHINE`, exit 1 |
| with an uncommitted change | "commit or stash first", exit 1, nothing modified |
| `shellcheck scripts/release.sh` | clean |

The first full run is the v0.1.0 release (phase 13); phase 12 dry-runs it in a throwaway worktree.
