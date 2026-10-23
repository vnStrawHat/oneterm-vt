# History, release dry run, user chapter

## `scripts/release.sh`: unchanged

Step order stays as shipped (0001 release-script.md, plus the `patch|minor|major` form): checks,
version bump, quality gate, `all --mib 32 --json` into `perf/history/v$V.json`, chart, commit,
tag. Phase 12 changes no line of it:

- It already builds with `--release`, the profile the targets are defined under (**D4**).
- `--json` now refuses a debug, `vt-paranoid` or `hotpath-profiling` build (check.md); the
  script uses none of them, so the guard only protects hand-made records.
- `grid --check` is not a script step. The owner runs it by hand before a release; once a
  release exists it compares against that record by default.

Recommended owner order before `scripts/release.sh` (goes into `docs/user/performance.md` and
perf/README "Commands"): quiet machine, then `vt-bench grid --check`; on FAIL, profile and fix
or record a waiver before releasing.

## First real record: `perf/history/v0.1.0.json`

Written by `scripts/release.sh 0.1.0` in phase 13 on the target machine, with `perf/chart.svg`
(one point per fixture until v0.2.0). Until then `perf/history/` does not exist and
`grid --check` refuses (refusal 5), which is correct: there is no baseline yet. Phase 12 never
commits a file under `perf/`.

## Release dry run (phase 12 exit, owner machine)

`release-script.md` asks phase 12 to dry-run the script. A worktree cannot hold `main` while the
main tree has it checked out, and `--force` would move the real `main` when the script commits.
So the dry run uses a local clone, inside the project, deleted afterwards:

```bash
git clone --quiet . .claude/worktrees/release-dry-run
cd .claude/worktrees/release-dry-run
VT_BENCH_MACHINE="<cpu, cores, os>" scripts/release.sh 0.1.0
git log --oneline -1 && git tag -l v0.1.0 && ls perf/history perf/chart.svg
cd ../../..
cargo run -p oneterm-vt-bench --release -- grid --check \
    --baseline .claude/worktrees/release-dry-run/perf/history/v0.1.0.json
rm -rf .claude/worktrees/release-dry-run
```

The clone holds the last commit, not the uncommitted phase-12 work. That is enough: the dry
run tests the script and the record it writes, and the `--check` above runs phase 12 code from
the main tree against that record. Commits and the tag stay in the clone; the shared
`CARGO_TARGET_DIR` reuses dependency builds. Pass = the script prints its final
`next: git push ...` line (so the record parsed: `chart` ran inside it) and the check prints PASS.
The outcome (pass, or the step that failed) goes into `acceptance.md`.

## `docs/user/performance.md` (chapter 12, <= 120 lines, no Rust blocks so no doctest wiring)

1. **What is measured**: the five tiers as a table (command, what it isolates, unit); tier 2 is
   the headline because it is what every byte costs an embedder.
2. **What is not measured**: no renderer, no pseudo-console, no threads or channels, one
   in-memory buffer, one machine; no cross-engine comparison.
3. **The numbers**: the tier-2 table from `acceptance.md` (machine, rustc, date named); phase 13
   replaces it with the v0.1.0 record. One sentence on `spread`: smaller differences are noise.
4. **Reproduce**: `cargo run -p oneterm-vt-bench --release -- grid --mib 32`, `all`, `--json
   --machine`; hygiene rules (release build, quiet machine, never pipe a generator in).
5. **History and chart**: `perf/history/v*.json` one per release, `perf/chart.svg`, one machine
   per comparison; link to `docs/perf/README.md` for the schema.
6. **The trip-wire**: `grid --check` purpose, default baseline, `--tolerance`, exit codes,
   refusals in one bullet list, why it never runs in CI; the passthrough-feature caveat.
7. **Profiling**: the hotpath recipe and the five sites (for contributors).
8. **Not a promise**: performance is outside the semver promise (`versioning.md`); a later
   version may be slower without a major bump; numbers are evidence about one build on one
   machine on one day.

Row 12 of `docs/user/README.md` becomes `[performance.md](performance.md)`, status `ready`.

## `docs/perf/README.md` edits

| Section | Change |
|---|---|
| Commands | add `grid --check` (by hand, baseline machine) and the two profile lines (hotpath.md) |
| new "Waivers" | after "Tier-2 targets" (targets.md format) |
| last line | tiers 3-5: "recorded per release, not targeted; compare releases on one machine" |
| Rules, bullet 3 | "(phase 12)" dropped from "The only verdict is `vt-bench grid --check`" |
