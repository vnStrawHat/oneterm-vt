# 0014: Release v0.1.0 (phase 13)

**Goal:** ship v0.1.0: the user guide complete and compiled, a committed public-API snapshot
gated in CI on Linux and Windows, three fuzz targets run clean, a headless example run in CI,
then the manual Release workflow runs `scripts/release.sh`, tests the release commit on 3 OS,
pushes commit and tag `v0.1.0` and publishes the GitHub release.

**Non-goals:** new public items; publishing to crates.io; fuzzing in CI; perf gates in CI beyond
the release trip-wire; a hand-written changelog; changes to `cliff.toml`; releases cut by hand.

**Precondition:** phases 0-12 are committed on `main` with their chapters.

| File | Covers |
|---|---|
| [user-docs.md](user-docs.md) | final chapter list, size/style rules, doctest wiring, `api-map.md`, `for-agents.md` |
| [public-api.md](public-api.md) | `scripts/public-api.sh`, the two snapshot files, pins, CI step, diff to bump kind |
| [examples.md](examples.md) | `examples/headless.rs`, the README quick start doctest |
| [fuzz.md](fuzz.md) | the `fuzz/` crate, three targets, the 10-minute run |
| [workflow.md](workflow.md) | `.github/workflows/release.yml` verbatim, guards |
| [release.md](release.md) | version input, `release.sh` changes, v0.1.0 checklist, failures, post-release |
| [tests.md](tests.md) | automated tests, CI steps, manual checks (M6 dry run), `acceptance.md` |
| [files.md](files.md) | every file touched, `lib.rs` edits, CI yaml, sub-milestones, commits |
| `acceptance.md` | written by coder and owner: snapshot review, fuzz runs, release verification |

## Acceptance criteria

- [ ] Gate, `--no-default-features` test, docs with `-D warnings` (files.md)
- [ ] Every `docs/user/*.md` and the root `README.md` is a doctest module; `tests/user_guide.rs` passes
- [ ] Both public-API snapshots committed; `scripts/public-api.sh check` green in CI on Linux and Windows
- [ ] Snapshot reviewed against the roadmap lists (M2); headless example green in CI on 3 OS
- [ ] Three fuzz targets x 10 min clean (fuzz.md), recorded in `acceptance.md`
- [ ] `release.yml` matches workflow.md; `release.sh` edits per release.md; shellcheck clean; M1
- [ ] M6 dry run green with nothing pushed; then checklist (release.md) done: tag `v0.1.0`, release with notes and two assets

## Decisions

1. v0.1.0 is cut with the workflow input `version` = `0.1.0` (already the workspace version).
2. Under 0.x, `major` gives `0.(x+1).0`; `minor` and `patch` give `0.x.(y+1)` (release.md).
3. Snapshot tool: `cargo-public-api 0.52.0` on `nightly-2026-05-25` (public-api.md).
4. `check` diffs the host's file only; `update` writes both via `--target` (public-api.md).
5. Perf: the phase-12 target table, measured by hand on the quiet i7-12700, is the quality verdict.
   The release record comes from the hosted runner; `grid --check` there is a 0.5-band trip-wire.
6. Fuzzing runs on Linux (WSL counts); cargo-fuzz does not support Windows. No CI job.
7. No new public item; `api-map.md`, `for-agents.md` and the example use roadmap items only.
8. `performance.md` keeps its phase-12 table and links `perf/history/`.
9. The phase commit is `docs(release)`; the workflow change is `ci(release)`: both hidden from notes.
10. Only the Release workflow releases; release.sh refuses outside GitHub Actions and never pushes.

## Design choices with alternatives (reviewer rules A = as proposed, B = the simpler alternative)

| # | Proposed (A) | Simpler alternative (B) | Evidence |
|---|---|---|---|
| D1 | `prepare` -> `test` x3 on the bundled release commit -> `publish` | one job; rely on CI green for the pre-bump commit, test matrix after the push | a pushed tag is never moved (commits.md); job-token pushes start no workflow (GitHub docs, "Triggering a workflow"), so under B the pushed commit is never built on Windows/macOS before it is public |
| D2 | release created by `publish` in the same run | push with a PAT so `on: push: tags` fires | same GitHub docs; a PAT is a long-lived secret to rotate |
| D3 | `git bundle` of the annotated tag as the artifact | upload the changed files, re-commit in `publish` | a re-commit has a new sha that no job tested |
| D4 | trip-wire baseline = previous release record; the new record on the first release | always the new record | a run against itself on one VM measures only noise |
| D5 | machine = runner label, OS, vCPU, CPU model, rustc | runner label only | the chart warns on differing `machine` strings (`vt-bench chart`); a hardware or compiler change should show |
| D6 | `dry_run` default `true` | default `false` | the tag cannot be taken back; one untick is the price |
| D7 | release.sh guard on `GITHUB_ACTIONS`; traps removed | docs-only rule; keep traps | the owner's rule becomes hard to break; the runner is discarded, nothing to undo |
| D8 | old `verify` job dropped | keep it | release.sh writes version, record and tag together in one run |

## Open items

1. **Repository URL (owner).** Replaces `<your fork url>` in `README.md` "Use" and the pin
   example in `versioning.md`, and is the `origin` the workflow pushes to (release.md step 2).
