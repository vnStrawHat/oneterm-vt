# 0014: Release v0.1.0 (phase 13)

**Goal:** ship v0.1.0: the user guide complete and compiled, a committed public-API snapshot
gated in CI on Linux and Windows, three fuzz targets run clean, a headless example run in CI,
then `scripts/release.sh 0.1.0` commits the perf record and creates tag `v0.1.0`.

**Non-goals:** new public items; publishing to crates.io; fuzzing or perf gates in CI; a
hand-written changelog; changes to `release.yml` or `cliff.toml`.

**Precondition:** phases 0-12 are committed on `main` with their chapters. The snapshot is only
meaningful once the last public item has landed; phase 13 does not write another phase's chapter.

| File | Covers |
|---|---|
| [user-docs.md](user-docs.md) | final chapter list, what this phase writes, size/style rules, doctest wiring, `api-map.md`, `for-agents.md` |
| [public-api.md](public-api.md) | `scripts/public-api.sh`, the two snapshot files, pins, CI step, update procedure, diff to bump kind |
| [examples.md](examples.md) | `examples/headless.rs`, the README quick start doctest |
| [fuzz.md](fuzz.md) | the `fuzz/` crate, three targets, the 10-minute run |
| [release.md](release.md) | v0.1.0 checklist in order, `release.sh` kinds under 0.x, post-release |
| [tests.md](tests.md) | automated tests, CI steps, manual checks, `acceptance.md` |
| [files.md](files.md) | every file touched, `lib.rs` edits, CI yaml, sub-milestones, commits |
| `acceptance.md` | written by coder and owner: snapshot review, fuzz runs, release verification |

## Acceptance criteria

- [ ] Gate, `cargo test -p oneterm-vt --no-default-features`, docs with `-D warnings` (files.md)
- [ ] Every `docs/user/*.md` and the root `README.md` is a doctest module; `tests/user_guide.rs` passes
- [ ] `crates/vt/public-api.{unix,windows}.txt` committed; `scripts/public-api.sh check` green in CI on Linux and Windows; shellcheck clean
- [ ] Snapshot reviewed against the roadmap item lists (tests.md M2), result in `acceptance.md`
- [ ] `cargo run -p oneterm-vt --example headless` green in CI on 3 OS
- [ ] Three fuzz targets x 10 min clean (fuzz.md), recorded in `acceptance.md`
- [ ] `release.sh` kind mapping verified (tests.md M1)
- [ ] `docs/user/README.md`: read-this-first block, every row linked; rows 00 and 13 written
- [ ] Release checklist (release.md) done: tag `v0.1.0`, GitHub release with notes and two assets

## Decisions

1. v0.1.0 is cut with `scripts/release.sh 0.1.0`: the workspace version already is `0.1.0`
   and the explicit form is used as given. No pre-bump commit, no script flag (release.md).
2. Under 0.x, `release.sh major` gives `0.(x+1).0`; `minor` and `patch` give `0.x.(y+1)`, as
   versioning.md and Cargo read 0.x versions; at 1.0 and later, plain semver (release.md).
3. Snapshot tool: `cargo-public-api 0.52.0` on `nightly-2026-05-25`, `--all-features
   --simplified`; one script for `check` and `update`, pins in one place (public-api.md).
4. `check` diffs the host's file only; `update` writes both files via `--target`, so one
   machine refreshes both (public-api.md).
5. v0.1.0 perf verdict = the phase-12 target table; `grid --check` has no baseline before the
   first record, and runs once against the new record before the push (release.md).
6. Fuzzing runs on Linux (WSL counts): cargo-fuzz does not support Windows. No CI job (fuzz.md).
7. No new public item: `api-map.md`, `for-agents.md` and the example use roadmap items only.
8. `performance.md` keeps its phase-12 table and links `perf/history/`; copying release
   numbers would need a commit after the tag (user-docs.md).
9. The phase commit is `docs(release)`, hidden from release notes by design: nothing an
   embedder compiles against changes (files.md).

## Open items

1. **Repository URL (owner).** Replaces `<your fork url>` in `README.md` "Use" and the pin
   example in `versioning.md`, and is the `origin` the tag is pushed to (release.md step 8).
