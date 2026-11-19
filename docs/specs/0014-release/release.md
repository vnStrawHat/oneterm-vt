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
| `bump <arg>` | `bump` | resolve `V`; print `releasing vOLD -> vV`; Actions guard; clean tree; on `main`; tag `vV` absent; `perf/history/{linux,windows,macos}/vV.json` and `perf/in-turns/.../vV.json` absent; rewrite `[workspace.package]` version; gate (fmt, clippy `--all-features`, test); `git add Cargo.toml Cargo.lock`; `git commit --allow-empty -m "release: vV"` | `bumped vV` |
| `bench` | `bench` x3 | `VT_BENCH_MACHINE` set; Actions guard; `P` = `RUNNER_OS` lower-cased, must be `linux`, `windows` or `macos`; `V` = workspace version; HEAD subject is `release: vV`; `perf/history/P/vV.json` and `perf/in-turns/P/vV.json` absent; `cargo test --workspace`; in turns, then record (below) | `recorded perf/history/P/vV.json` |
| `commit` | `finalize` | Actions guard; `V`; HEAD subject `release: vV`; tag `vV` absent; the three records exist; when an earlier tag exists, the three in-turns records exist and `vt-bench chart --in-turns perf/in-turns/P/vV.json --out perf/chart-P.svg` for each `P`; `git status --porcelain -- . ':(exclude)perf'` empty; `git add perf`; `git commit --amend --no-edit`; `git tag -a vV -m "oneterm-vt vV"` | `tagged vV` |

`bench` measures the last five releases beside this build, all in turns in the same job
(replaced after v0.4.2: a run held against a stored record failed two release runs on runner
speed alone, and a chart of one record per release drew the runners; 0013 check.md):

```bash
cargo test --workspace
in_turns; record
# in_turns: tags=$(git tag --list 'v[0-9]*' --merged HEAD --sort=v:refname | tail -n 5)   # none: nothing to run
#           git worktree add --detach .tmp/previous/src <newest>; then per tag, oldest first:
#             git -C .tmp/previous/src checkout --detach "$tag"
#             (cd .tmp/previous/src && RUSTUP_TOOLCHAIN=<this job's> cargo build -p oneterm-vt-bench --release --target-dir ../target)
#             cp .tmp/previous/target/release/vt-bench[.exe] .tmp/previous/vt-bench-$tag[.exe]
#           cargo run -p oneterm-vt-bench --release -- grid --check --against <each copy, oldest first> \
#               --tolerance "${VT_BENCH_THROUGHPUT_TOLERANCE:-0.75}" --machine "$VT_BENCH_MACHINE" \
#               --out "perf/in-turns/$P/v$V.json" | tee -a "$GITHUB_STEP_SUMMARY"
# record:   mkdir -p "perf/history/$P" && cargo run -p oneterm-vt-bench --release -- all --mib 32 --json \
#               --machine "$VT_BENCH_MACHINE" --out "perf/history/$P/v$V.json" | tee -a "$GITHUB_STEP_SUMMARY"
```

- One worktree and one target folder serve every tag: what the tags share is built once.
- The newest tag must build, or the job fails; an older one that does not build with this
  toolchain is left out with a warning in the summary.
- `HISTORY=5` in the script: each earlier release costs the job a build and its share of every
  round.
- The workflow input `throughput_tolerance` (default `0.75`) reaches the script as
  `VT_BENCH_THROUGHPUT_TOLERANCE`.
- Each bench job uploads two artifacts, `record-P` and `in-turns-P`; `finalize` copies both under
  `perf/` before `commit`.

With `--json` and `--out` the record goes to the file and stdout carries the markdown report of
the same run (tiers 1-5), so the job summary shows the trip-wire table, then the full measurement.

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
| 10 | `gh release view v0.1.0` | title `v0.1.0`; notes grouped Features / Bug fixes / Performance, one line per phase commit, no `docs`/`ci`/`release` lines; no assets besides GitHub's source archives |
| 11 | `git show v0.1.0:perf/history/P/v0.1.0.json` and `git show v0.1.0:perf/chart-P.svg` for each `P`; the README charts render on GitHub | all six exist; both panels show |
| 12 | Record tag sha, release URL and run ids in `acceptance.md` | done |

Steps 3-6 cover what the workflow does not: clippy, docs and the snapshot on 3 OS (CI), hostile
input (fuzz), the target verdict on a quiet machine.

## When something fails

- `bump`, `bench` or `finalize` red: nothing was pushed. Fix forward on `main`, run again. A red
  trip-wire: read that job's summary table. Every build ran in the same job, so a retry
  ("Re-run failed jobs", the run keeps its `bump` artifact) is for a ratio near the band
  only. A real regression is fixed first; a cost that is meant is released with the
  tolerance input raised and the reason in the commit.
- After the push: never move, delete or re-push a pushed tag (`docs/agents/commits.md`). Fix
  forward and release `0.1.1` with kind `patch`.
- Red `publish` with no code at fault: "Re-run failed jobs". The artifact belongs to the run, a
  push of refs that already match is a no-op, then `gh release create` runs only if `gh release view "$TAG"` finds no release.

## Post-release

- Pull after every release: the workflow adds a commit to `main`.
- `workspace.package.version` stays at the released version until the next release; no "-dev"
  bump. Between releases, `XTVERSION` from `main` says the last release; accepted.
- Next release: kind = highest bump among commits since the last tag (commits.md), mapped per
  the table above. Each platform's job then measures the last five releases beside the new build.
- `git diff v0.1.0 -- crates/vt/public-api.*.txt` is the surface change of the next release.
