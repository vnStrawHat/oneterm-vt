# Release v0.1.0: script, checklist, post-release

## Which version (Decision 1)

The workspace version is already `0.1.0` and no tag exists, so the first run uses the Release
workflow's `version` input `0.1.0`: release.sh uses an explicit `X.Y.Z` as given, rewrites the
same line, runs the gate, writes the record and chart, commits `release: v0.1.0` and tags
`v0.1.0`. A kind (`minor`) would skip `0.1.0` itself; a "pre-bump" commit would be churn.

## `scripts/release.sh` (Decisions 2, 10)

Kinds under 0.x (already shipped in 13d):

| Current | `patch` | `minor` | `major` |
|---|---|---|---|
| `0.x.y` | `0.x.(y+1)` | `0.x.(y+1)` | `0.(x+1).0` |
| `M.x.y`, `M >= 1` | `M.x.(y+1)` | `M.(x+1).0` | `(M+1).0.0` |

Changes for the workflow (13f), nothing else:

| Where | Change |
|---|---|
| header, lines 2-3 | "Release steps run by `.github/workflows/release.yml`: bump, gate, bench, chart, commit, tag. It never pushes; the workflow pushes after the 3-OS test." |
| usage text | `VT_BENCH_MACHINE` example `"<runner, os, cores, cpu, rustc>"` |
| after `echo "releasing ..."` | `[ "${GITHUB_ACTIONS:-}" = true ] \|\| fail "releases are cut by the Release workflow (.github/workflows/release.yml)"` |
| both `trap ... ERR` lines and their comments | removed: the runner is discarded, there is nothing to undo |
| last line | `echo "tagged v$V"` (replaces the `git push` hint) |

The guard sits after the mapping line, so M1 (tests.md) still reads the mapping locally.

## Checklist (in order; owner)

| # | Step | Pass when |
|---|---|---|
| 1 | Phases 0-13 and 13f committed on `main`; `git status` clean | every spec row in `docs/README.md`; every row in `docs/user/README.md` linked |
| 2 | Repository URL decided (open item 1); `git remote add origin <url>`. Repository settings: no branch or tag rule on `main` / `v*` blocks pushes by `github-actions[bot]` | README "Use" and the `versioning.md` pin carry the URL |
| 3 | `git push origin main`; wait for CI | `check` green on Linux, Windows, macOS, incl. public-API snapshot and headless example |
| 4 | Public-API review | tests.md M2 recorded in `acceptance.md`; no unlisted root item |
| 5 | Fuzz | `parser`, `feed`, `resize` 10 min each, clean, at this commit (fuzz.md); recorded |
| 6 | Perf verdict, quiet i7-12700 | `docs/specs/0013-perf/acceptance.md`: all ten tier-2 fixtures pass or carry a waiver |
| 7 | Dry run: Actions tab, Release, "Run workflow" from `main`, version `0.1.0`, `dry_run` ticked (or `gh workflow run release.yml -f version=0.1.0 -f dry_run=true`) | `prepare` and `test` x3 green, `publish` skipped; summary ends `PASS: 10 of 10 ...`; notes list the phase commits; artifact `release` holds 4 files; `git ls-remote --tags origin` has no `v0.1.0` |
| 8 | Real run: same inputs, `dry_run` unticked | all jobs green, `publish` included |
| 9 | `git pull --ff-only` | local `main` at `release: v0.1.0` by `github-actions[bot]`, tag `v0.1.0` on it |
| 10 | `gh release view v0.1.0` | title `v0.1.0`; notes grouped Features / Bug fixes / Performance, one line per phase commit, no `docs`/`ci`/`release` lines; assets `chart.svg`, `v0.1.0.json` |
| 11 | `gh release download v0.1.0 -p '*.json' -D .tmp/release-check`, `cmp` with `perf/history/v0.1.0.json` | identical |
| 12 | Record tag sha, release URL and run ids in `acceptance.md` | done |

Steps 3-6 cover what the workflow does not: clippy, docs and the snapshot on 3 OS (CI), hostile
input (fuzz), the target verdict on a quiet machine.

## When something fails

- `prepare` or `test` red: nothing was pushed. Fix forward on `main`, run again.
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
  the table above. The trip-wire then compares with the previous release record.
- `git diff v0.1.0 -- crates/vt/public-api.*.txt` is the surface change of the next release.
