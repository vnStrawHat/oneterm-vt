# 0014: Release v0.1.0 (phase 13)

**Goal:** ship v0.1.0: the user guide complete and compiled, a committed public-API snapshot
gated in CI on Linux and Windows, three fuzz targets run clean, a headless example run in CI,
then the manual Release workflow bumps the version, tests and benchmarks that commit on Linux,
Windows and macOS (one perf record and chart per platform), pushes commit and tag `v0.1.0` and
publishes the GitHub release.

**Non-goals:** new public items; publishing to crates.io; fuzzing in CI; perf gates in CI beyond
the release trip-wire; a hand-written changelog; changes to `cliff.toml`; releases cut by hand;
comparing perf records across platforms.

**Precondition:** phases 0-12 are committed on `main` with their chapters.

| File | Covers |
|---|---|
| [user-docs.md](user-docs.md) | final chapter list, size/style rules, doctest wiring, `api-map.md`, `for-agents.md` |
| [public-api.md](public-api.md) | `scripts/public-api.sh`, the two snapshot files, pins, CI step, diff to bump kind |
| [examples.md](examples.md) | `examples/headless.rs`, the README quick start doctest |
| [fuzz.md](fuzz.md) | the `fuzz/` crate, three targets, the 10-minute run |
| [platforms.md](platforms.md) | 13g: platform key, `perf/` layout, record field `platform`, `vt-bench` changes and tests |
| [workflow.md](workflow.md) | `.github/workflows/release.yml` verbatim (bump, bench x3, finalize, publish), guards |
| [release.md](release.md) | version input, `release.sh` sub-commands, v0.1.0 checklist, failures, post-release |
| [tests.md](tests.md) | automated tests, CI steps, manual checks (M1, M6, M7), `acceptance.md` |
| [files.md](files.md) | every file touched, `lib.rs` edits, CI yaml, sub-milestones, commits |
| `acceptance.md` | written by coder and owner: snapshot review, fuzz runs, release verification |

## Acceptance criteria

- [ ] Gate, `--no-default-features` test, docs with `-D warnings` (files.md)
- [ ] Every `docs/user/*.md` and the root `README.md` is a doctest module; `tests/user_guide.rs` passes
- [ ] Both public-API snapshots committed; `scripts/public-api.sh check` green in CI on Linux and Windows
- [ ] Snapshot reviewed against the roadmap lists (M2); headless example green in CI on 3 OS
- [ ] Three fuzz targets x 10 min clean (fuzz.md), recorded in `acceptance.md`
- [ ] 13g: `vt-bench` per platforms.md with its tests; `release.sh` sub-commands per release.md; `release.yml` matches workflow.md; shellcheck clean; M1, M7
- [ ] M6 dry run green with nothing pushed; then checklist (release.md) done: tag `v0.1.0`, release with notes and only the source archives GitHub attaches

## Decisions

1. v0.1.0 is cut with the workflow input `version` = `0.1.0` (already the workspace version).
2. Under 0.x, `major` gives `0.(x+1).0`; `minor` and `patch` give `0.x.(y+1)` (release.md).
3. Snapshot tool: `cargo-public-api 0.52.0` on `nightly-2026-05-25` (public-api.md).
4. `check` diffs the host's file only; `update` writes both via `--target` (public-api.md).
5. Perf: the phase-12 target table, measured by hand on the quiet i7-12700, is the quality verdict.
   Release records come from the three hosted runners; `grid --check` on each is a 0.5-band trip-wire
   against that platform's previous record. Records are compared only within one platform.
6. Fuzzing runs on Linux (WSL counts); cargo-fuzz does not support Windows. No CI job.
7. No new public item; `api-map.md`, `for-agents.md` and the example use roadmap items only.
8. `performance.md` keeps its phase-12 table and links the per-platform history and charts.
9. The phase commit is `docs(release)`; the workflow changes are `ci(release)`: hidden from notes.
10. Only the Release workflow releases; release.sh refuses outside GitHub Actions and never pushes.

## Design choices with alternatives (reviewer rules A = as proposed, B = the simpler alternative)

| # | Proposed (A) | Simpler alternative (B) | Evidence |
|---|---|---|---|
| D1 | `bump` -> `bench` x3 -> `finalize` -> `publish` | bench on Linux only, test x3 after | owner request 2026-10-26 (bench on 3 platforms); a pushed tag is never moved (commits.md); job-token pushes start no workflow (GitHub docs, "Triggering a workflow"), so the tree must be tested on 3 OS before the push |
| D2 | release created by `publish` in the same run | push with a PAT so `on: push: tags` fires | same GitHub docs; a PAT is a long-lived secret to rotate |
| D3 | `git bundle` of the bump commit to `bench`/`finalize`, of the tag to `publish` | a `git diff` patch artifact, or re-running the bump in every job | bundles carry exact objects (git-bundle docs); a patch meets CRLF work trees on Windows checkouts; three re-run bumps are equal only by assumption |
| D4 | trip-wire baseline = the platform's previous record; the new record on a platform's first release | always the new record | a run against itself on one VM measures only noise |
| D5 | machine = runner label, OS and version, vCPU, CPU model, rustc, per runner | runner label only | the chart warns on differing `machine` strings (`vt-bench chart`); an image or compiler change should show |
| D6 | `dry_run` default `true` | default `false` | the tag cannot be taken back; one untick is the price |
| D7 | release.sh guard on `GITHUB_ACTIONS`; traps removed | docs-only rule; keep traps | the owner's rule becomes hard to break; the runner is discarded, nothing to undo |
| D8 | old `verify` job dropped | keep it | `finalize` writes records, charts and tag together |
| D9 | no second 3-OS test of the amended commit | test x3 again after `finalize` | the amend adds only `perf/` (`release.sh commit` refuses anything else); no build script, `include_*!` or test reads `perf/` (`grid --check` reads it at run time only) |
| D10 | record field `platform` plus a folder per platform | folder only | a misfiled or copied-out record still names its platform; `vt-bench` refuses foreign baselines and mixed charts (platforms.md) |
| D11 | `perf/chart-<platform>.svg`, flat | `perf/<platform>/chart.svg` | one flat name per platform reads well in the root README, which embeds the three charts |
| D12 | trip-wire runs before the record exists, default baseline | `find` + `sort -V` for the previous file | `vt-bench` already sorts by version (0013 check.md D1); GNU and BSD `sort` differ across the three runners |
| D13 | the release carries no assets of ours; records and charts are read at the tag in the repository | upload charts and records as assets | owner request 2026-10-26; the release commit already holds them, so assets would duplicate files |
| D14 | `bench` prints the `all` report (markdown) to stdout when `--json` has `--out`; `tee -a` puts it in the job summary | measure twice, or print a second table by hand | one measurement, two views; tier 1 was invisible in the summary |
| D15 | `chart` draws tier 1 above tier 2 in one SVG; the trip-wire stays tier 2 | tier 1 in a second file; a tier-1 band | one file per platform keeps the README short; 0013 D3: tier-1 spreads are too noisy for a band |

## Open items

None. The repository is `https://github.com/vnStrawHat/oneterm-vt` (`origin`, the remote the workflow pushes to).
