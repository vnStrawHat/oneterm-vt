# Release v0.1.0: script, checklist, post-release

## Which version (Decision 1)

The workspace version is already `0.1.0` and no tag exists, so the first run uses the Release
workflow's `version` input `0.1.0`: `release.sh bump` uses an explicit `X.Y.Z` as given and
rewrites the same line (no change, so the bump commit is empty until `commit` adds `perf/`).
A kind (`minor`) would skip `0.1.0` itself; a "pre-bump" commit would be churn.

## `scripts/release.sh` (Decisions 2, 10; split into sub-commands in 13g)

Kinds under 0.x (shipped in 13d):

| Current | `patch` | `minor` | `major` |
|---|---|---|---|
| `0.x.y` | `0.x.(y+1)` | `0.x.(y+1)` | `0.(x+1).0` |
| `M.x.y`, `M >= 1` | `M.x.(y+1)` | `M.(x+1).0` | `(M+1).0.0` |

```text
usage: scripts/release.sh bump <patch|minor|major|X.Y.Z>
       VT_BENCH_MACHINE="<runner, os, cores, cpu, rustc>" scripts/release.sh bench
       scripts/release.sh commit
```

Any other shape: usage, exit 2. Header: "Release steps run by `.github/workflows/release.yml`,
one sub-command per job: bump, bench (on each OS), commit. It never pushes; the workflow pushes
after every job is green." Every sub-command refuses outside Actions (`[ "${GITHUB_ACTIONS:-}" =
true ] || fail "releases are cut by the Release workflow (.github/workflows/release.yml)"`).

| Sub-command | Job | Steps, in order | Last line |
|---|---|---|---|
| `bump <arg>` | `bump` | resolve `V`; print `releasing vOLD -> vV`; Actions guard; clean tree; on `main`; tag `vV` absent; `perf/history/{linux,windows,macos}/vV.json` absent; rewrite `[workspace.package]` version; gate (fmt, clippy `--all-features`, test); `git add Cargo.toml Cargo.lock`; `git commit --allow-empty -m "release: vV"` | `bumped vV` |
| `bench` | `bench` x3 | `VT_BENCH_MACHINE` set; Actions guard; `P` = `RUNNER_OS` lower-cased, must be `linux`, `windows` or `macos`; `V` = workspace version; HEAD subject is `release: vV`; `perf/history/P/vV.json` absent; `cargo test --workspace`; trip-wire and record (below) | `recorded perf/history/P/vV.json` |
| `commit` | `finalize` | Actions guard; `V`; HEAD subject `release: vV`; tag `vV` absent; the three records exist; `vt-bench chart --history perf/history/P --out perf/chart-P.svg` for each `P`; `git status --porcelain -- . ':(exclude)perf'` empty; `git add perf`; `git commit --amend --no-edit`; `git tag -a vV -m "oneterm-vt vV"` | `tagged vV` |

`bench` runs the trip-wire before the record when the platform already has one: the newest
record of the folder is then the previous release's, and `vt-bench` picks it by version (no
shell `sort -V`, whose behaviour differs between GNU and BSD `sort`):

```bash
cargo test --workspace
if [ -d "perf/history/$P" ]; then trip_wire; record; else record; trip_wire; fi
# trip_wire: cargo run -p oneterm-vt-bench --release -- grid --check | tee -a "$GITHUB_STEP_SUMMARY"
# record:    mkdir -p "perf/history/$P" && cargo run -p oneterm-vt-bench --release -- all --mib 32 --json \
#                --machine "$VT_BENCH_MACHINE" --out "perf/history/$P/v$V.json"
```

The first record of a platform is compared with itself (Decision D4): it proves the record parses
and the check runs on that OS. Shared helpers: `fail`, `usage`, `workspace_version` (the
existing awk), `require_actions`, `require_release_head`. Bash 3.2-compatible (macOS `/bin/bash`):
lower-case with `tr '[:upper:]' '[:lower:]'`, no `${x,,}`, no `mapfile`. No traps, never pushes.

## Checklist (in order; owner)

| # | Step | Pass when |
|---|---|---|
| 1 | Phases 0-13, 13f and 13g committed on `main`; `git status` clean | every spec row in `docs/README.md`; every row in `docs/user/README.md` linked |
| 2 | `origin` is `https://github.com/vnStrawHat/oneterm-vt`. Repository settings: no branch or tag rule on `main` / `v*` blocks pushes by `github-actions[bot]` | README "Use" and the `versioning.md` pin carry the URL |
| 3 | `git push origin main`; wait for CI | `check` green on Linux, Windows, macOS, incl. public-API snapshot and headless example |
| 4 | Public-API review | tests.md M2 recorded in `acceptance.md`; no unlisted root item |
| 5 | Fuzz | `parser`, `feed`, `resize` 10 min each, clean, at this commit (fuzz.md); recorded |
| 6 | Perf verdict, quiet i7-12700 | `docs/specs/0013-perf/acceptance.md`: all ten tier-2 fixtures pass or carry a waiver |
| 7 | Dry run: Actions tab, Release, "Run workflow" from `main`, version `0.1.0`, `dry_run` ticked (or `gh workflow run release.yml -f version=0.1.0 -f dry_run=true`) | tests.md M6 |
| 8 | Real run: same inputs, `dry_run` unticked | all jobs green, `publish` included |
| 9 | `git pull --ff-only` | local `main` at `release: v0.1.0` by `github-actions[bot]`, tag `v0.1.0` on it; `git show --stat HEAD` lists 3 records and 3 charts |
| 10 | `gh release view v0.1.0` | title `v0.1.0`; notes grouped Features / Bug fixes / Performance, one line per phase commit, no `docs`/`ci`/`release` lines; assets `chart-{linux,windows,macos}.svg`, `v0.1.0-{linux,windows,macos}.json` |
| 11 | `gh release download v0.1.0 -p '*.json' -D .tmp/release-check`; `cmp` each `v0.1.0-P.json` with `perf/history/P/v0.1.0.json` | all three identical |
| 12 | Record tag sha, release URL and run ids in `acceptance.md` | done |

Steps 3-6 cover what the workflow does not: clippy, docs and the snapshot on 3 OS (CI), hostile
input (fuzz), the target verdict on a quiet machine.

## When something fails

- `bump`, `bench` or `finalize` red: nothing was pushed. Fix forward on `main`, run again. A red
  trip-wire on one platform: read that job's summary table; a noisy runner may be retried with
  "Re-run failed jobs" (the run keeps its `bump` artifact); a real regression is fixed first.
- After the push: never move, delete or re-push a pushed tag (`docs/agents/commits.md`). Fix
  forward and release `0.1.1` with kind `patch`.
- Red `publish` with no code at fault: "Re-run failed jobs". The artifact belongs to the run, a
  push of refs that already match is a no-op, then `gh release create` runs only if `gh release view "$TAG"` finds no release.
  If the release already exists, upload a missing asset with `gh release upload "$TAG" <file>` instead of re-creating.

## Post-release

- Pull after every release: the workflow adds a commit to `main`.
- `workspace.package.version` stays at the released version until the next release; no "-dev"
  bump. Between releases, `XTVERSION` from `main` says the last release; accepted.
- Next release: kind = highest bump among commits since the last tag (commits.md), mapped per
  the table above. Each platform's trip-wire then compares with that platform's previous record.
- `git diff v0.1.0 -- crates/vt/public-api.*.txt` is the surface change of the next release.
