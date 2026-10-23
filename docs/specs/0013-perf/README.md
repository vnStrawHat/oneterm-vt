# 0013: Perf targets (phase 12)

**Goal:** every tier-2 fixture meets its target in [docs/perf/README.md](../../perf/README.md)
on the target machine, or carries an owner-approved waiver; `hotpath-profiling` measures five
hot functions; `vt-bench grid --check` is a by-hand regression trip-wire against the newest
`perf/history` record; user chapter 12 explains all of it.

**Non-goals:** CI perf gates; targets for tiers 3-5; a criterion/`benches/` harness; new public
items; changing `scripts/release.sh`; committing anything under `perf/` (phase 13 does).

| File | Covers |
|---|---|
| [targets.md](targets.md) | measurement rules, where we stand, miss procedure (profile, fix, waiver), waiver record |
| [check.md](check.md) | `grid --check` flags, default baseline, refusals, verdict, exact output, exit codes, why never CI |
| [hotpath.md](hotpath.md) | feature wiring, the five sites, no-op build, bench feature and guard, `--fixture`, recipe, CI |
| [history.md](history.md) | release.sh unchanged, first record, release dry run, `docs/user/performance.md` outline, perf/README edits |
| [tests.md](tests.md) | unit tests, build checks, manual checks |
| [files.md](files.md) | files, USAGE, rules, gate, commit order |
| `acceptance.md` | written by the coder: runs, profiles, fixes, waivers, dry run |

## Acceptance criteria

- [ ] Gate in files.md green (fmt, clippy incl. the off-half line, tests, `--no-default-features`, docs)
- [ ] Five `hotpath::measure` sites; manual checks 6-7 show them in a report
- [ ] `grid --check`: every refusal, the exact table, exit codes 0/1/2 (tests.md, manual 1-5)
- [ ] `--json` refused in debug / `vt-paranoid` / `hotpath-profiling` builds
- [ ] All ten tier-2 fixtures pass (>= 0.90 x target, median of three runs) or have a waiver row
- [ ] Release dry run in a clone passes; `acceptance.md` filled; `performance.md` row 12 `ready`
- [ ] Commits per files.md: `feat(bench): grid --check trip-wire and hotpath-profiling sites`, then `perf(...)` fixes

## Decisions (settled from roadmap, docs/perf/README, specs 0001/0002/0007, hotpath and cargo docs)

1. The check covers tier 2 only, runs by hand on the baseline machine, never in CI.
2. Fail strictly below `tolerance x baseline`; exactly at the band passes; default 0.5.
3. Missing fixture on either side, wrong geometry, empty history: refusal (exit 2), never a skip.
4. `--mib` defaults to the baseline's `mib_per_fixture`, so both sides use the same input size.
5. Sites per chunk, per frame or per scrolled line only; never per byte or per cell.
6. Target verdict: median of three runs, spread <= 15%, `sixel` included.
7. Fixes are separate `perf` commits; agents never approve waivers.

## Design choices with alternatives (A = as proposed, B = the alternative)

| # | A | B | Evidence |
|---|---|---|---|
| D1 | baseline = newest `perf/history/v*.json`; `--baseline FILE` for anything else | a separate committed baseline file | each release record already is a full tier-2 run from the baseline machine with the same command, under perf/README's never-edit rule; B duplicates it and drifts |
| D2 | exit 0 pass, 1 regression, 2 no verdict | 0/1 only | a script can tell "slower" from "could not judge"; 2 already means usage error in this CLI (0001 bench-cli.md) |
| D3 | tolerance 0.5 (factor band) | 0.9 (the target band) | our own spreads: 2-11% tier 2 (0007), 18-62% tier 1 on a busy desktop (0002); a 10% band trips on noise, a factor band catches structural slowdowns |
| D4 | measure with `[profile.release]` (thin LTO, `codegen-units = 1`) | a `perf` profile inheriting release with `lto = "fat"`, used by release.sh | targets are defined under release; embedders compile us with their own profile, so B would publish numbers nobody gets; B adds a second full build per release run (cargo profiles docs: custom profiles build into their own directory); B's gain is unmeasured |
| D5 | switch = bench cargo feature + `hotpath::main` on `main` | a `--profile` runtime flag | sites exist only in a binary compiled with `hotpath/hotpath` (hotpath docs), so B still needs the feature and adds a flag |
| D6 | `--fixture NAME` on parser/grid/render | profile all ten fixtures in one process | hotpath aggregates per function per process; B cannot attribute time to a fixture |
| D7 | refusals read bench passthrough features | a public `oneterm_vt` const exposing build features | B adds a public item outside the roadmap lists (phase 13 API snapshot); A's bypass via `--features oneterm-vt/...` is documented |

Rows are independent: B on a row reverts only that row's rules and tests.

## Open items

None. Which fixtures miss after phases 7-11, and fix-or-waive for each, is decided during 12c
by measurement and the owner (targets.md), not by this spec.
