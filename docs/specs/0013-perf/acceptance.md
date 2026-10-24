# 0013 acceptance notes (phase 12)

Machine: Intel Core i7-12700 (12C/20T), Windows 11 build 26300, rustc 1.96.0, 2026-10-24.
Method: `vt-bench grid --mib 32` from a `--release` build, started with `start /b /wait /high
/affinity 10` (one performance core, high priority). Unpinned, this desktop gave 20-86% spreads
on every fixture (hybrid-CPU core migration plus background load), so the 15% rule could not be
met without pinning. A run with any spread above 15% is discarded; up to 14 attempts per table.

## Tier 2 targets (median of three runs, MiB/s; pass at >= 0.90 x target)

Before = build without the parser fix below; after = with it. Spread = worst of the three runs.

| Fixture | Target | Before: runs | Median | Ratio | After: runs | Median | Spread | Ratio | Verdict |
|---|---:|---|---:|---:|---|---:|---:|---:|---|
| `plain_ascii` | 75 | 149.5 147.6 148.4 | 148.4 | 1.98 | 157.3 154.5 144.8 | 154.5 | 13% | 2.06 | pass |
| `long_lines` | 81 | 157.8 158.5 153.6 | 157.8 | 1.95 | 161.5 162.2 153.6 | 161.5 | 11% | 1.99 | pass |
| `heavy_sgr` | 227 | 183.4 183.6 180.8 | 183.4 | 0.81 | 262.8 263.7 251.1 | 262.8 | 7% | 1.16 | pass |
| `tui_redraw` | 126 | 166.3 162.8 160.3 | 162.8 | 1.29 | 199.3 199.3 186.1 | 199.3 | 10% | 1.58 | pass |
| `scroll_region` | 61 | 87.5 85.0 86.0 | 86.0 | 1.41 | 90.3 91.2 89.5 | 90.3 | 10% | 1.48 | pass |
| `cjk_wide` | 115 | 135.2 137.8 136.8 | 136.8 | 1.19 | 142.5 142.3 134.9 | 142.3 | 12% | 1.24 | pass |
| `dense_cells` | 196 | 150.8 151.9 151.4 | 151.4 | 0.77 | 187.5 188.7 183.1 | 187.5 | 11% | 0.96 | pass |
| `scrolling` | 80 | 160.0 160.4 159.6 | 160.0 | 2.00 | 161.9 164.3 156.8 | 161.9 | 14% | 2.02 | pass |
| `sixel` | 47 | 41.7 41.4 41.4 | 41.4 | 0.88 | 42.7 42.7 41.5 | 42.7 | 14% | 0.91 | pass, thin |
| `osc_9_7` | 102 | 163.2 162.5 161.1 | 162.5 | 1.59 | 160.3 163.0 156.6 | 160.3 | 11% | 1.57 | pass |

Before: two runs within 15% and one at 18% (`plain_ascii` 18%, `dense_cells` 16%); the other 11
attempts were discarded for spreads of 27-86%. After: three runs within 15% out of 11 attempts.
`sixel` passes by 0.9 MiB/s: its pass line is 42.3, and single-fixture runs (`grid --fixture
sixel`) gave 41.4-43.7 across sessions. Treat it as an owner decision when it flips (below).

## Profiles

No sampling profiler exists on this machine, so the hotpath sites (8 MiB, one fixture) were
combined with experiments. `hotpath` timing is `vt_bench::main` 100%; the sites nest.

| Fixture (build) | feed | advance | scroll_up | other |
|---|---|---|---|---|
| `heavy_sgr` (before) | 272.7 ms | 272.6 ms | 16 536 calls, 0.55 ms | main 676.4 ms (stream generation outside the timer) |
| `dense_cells` (after) | 260.1 ms | 260.0 ms | never reached | main 670.4 ms |
| `sixel` (after) | 1.18 s | 1.17 s | 718 764 calls, 133.5 ms | main 1.24 s |
| `dense_cells` via `render --frames 100` | 12.25 ms | 12.11 ms | - | `snapshot_update` 606 calls, avg 5.4 us, 3.28 ms |

Findings on `heavy_sgr` and `dense_cells` (single-fixture runs, same core): the parser alone
(tier 1) runs `heavy_sgr` at 352 MiB/s and `dense_cells` at 328, the whole engine at 196 and
161, so more than half of the time was in `advance`, none in the grid. Skipping the SGR handler
(temporary experiment) gave 228 and 208 MiB/s, so SGR dispatch is about a fifth. The parser is
therefore where a fix pays: every parameter byte went through the whole transition table
(`step`, then `anywhere`, then `header` with its family and phase match).

Findings on `sixel`: temporary `measure_block!` sites (hotpath overhead included, 100 ns
resolution) per image of about 80 bytes: `place` 989 ns (of which `release_covered` 285,
cursor walk and stamping 96, eviction at the 256-placement cap 71, `push_pending` 30), `finish`
175, decoder construction 80, scroll 182. `release_covered` scans every live placement per
image; removing it (experiment) gave 50 MiB/s. The tier-1 parser handles the same stream at
720 MiB/s, so the cost is placement bookkeeping and the decoder, spread over many small steps.

## Fixes

| Commit scope | Files | Hunks |
|---|---|---|
| `perf(parser)` | `crates/vt/src/parser/state.rs`, `crates/vt/src/parser/mod.rs` | `csi_parameter` and `parameter_byte` in state.rs (and `header` calling `parameter_byte`); the CSI branch in `Parser::advance` in mod.rs. The `cfg_attr` line on `advance` belongs to the tooling commit. |

`perf(parser)`: a digit, `;` or `:` in a CSI header that is still in `CsiEntry` or `CsiParam`
skips the table and goes straight to `digit` / `separator`; every other byte takes the same
path as before. Single-fixture, same core (median of three): `heavy_sgr` 195.1 to 261.8,
`dense_cells` 161.6 to 187.9, `tui_redraw` 174.2 to 191.4, `osc_9_7` 173.1 to 161.5, every
other fixture within 2%. `cargo test -p oneterm-vt`: 1014 passed, the chunking and malformed
header tests included.

Placement A/B (three interleaved runs per build, medians, MiB/s; base = HEAD parser): the branch
in `Parser::advance`'s loop vs the same branch as the first arm of `step`. `heavy_sgr` 261.8 vs
250.7 (a second pass: 263.1 vs 254.0), `dense_cells` 187.9 vs 186.5, `tui_redraw` 191.4 vs 188.5,
`osc_9_7` 161.5 vs 168.4 (base 173.1). The first arm recovers most of `osc_9_7` (the loop's extra
branch runs on every non-ground payload byte, OSC and DCS included, and the first arm moves that
cost into `step`'s match) but costs `heavy_sgr` 4%, over the 3% limit, so the loop placement stays.

Tried and dropped, no gain beyond noise on `sixel`: a tighter `release_covered` scan (+1-2%) and
a one-pixel store in the decoder's `fill` (0%). Both were reverted.

## Pending owner decisions

- `sixel` clears its pass line by 1% (42.7 against 42.3). Real gains need several small
  changes (placement bookkeeping, per-byte decoder, allocations); none was attempted. If the
  owner wants margin, that is its own `perf` item.
- The 15% spread rule needs a pinned core on this machine; the owner may prefer to run the
  final numbers on a quieter one.
- No waiver is proposed (`dense_cells` 0.96 and `sixel` 0.91 pass); the Waivers table in `docs/perf/README.md` stays empty.

## One `all --mib 32` run (pinned, one run, unfiltered)

Tier 2 (MiB/s): plain_ascii 140.2, long_lines 142.7, heavy_sgr 239.8, tui_redraw 177.5,
scroll_region 83.4, cjk_wide 130.5, dense_cells 172.1, scrolling 163.4, sixel 43.5, osc_9_7
147.6 (spreads 9-51%: this run is a record, not a verdict). Tier 1 (MiB/s): 1125, 1264, 469,
510, 937, 893, 429, 1205, 654, 544. Tier 3 (us/frame): 4.3, 2.3, 1.1, 16.5, 20.7, 1.6, 1.5,
2.2, 4.6, 5.0. Tier 4 (grow/shrink us): 3.2/2.3 at 0 rows, 567/516 at 10 000, 11 614/8 758 at
100 000. Tier 5: 1364 bytes per row for plain, unicode, styled and mixed.

## Manual checks (tests.md)

| # | Result |
|---|---|
| 1 | no `perf/history`: `vt-bench: no perf record in perf/history; scripts/release.sh writes the first, or pass --baseline FILE`, exit 2 |
| 2 | debug build: `--check needs a release build (cargo run --release)`, exit 2 |
| 3 | `--features vt-paranoid` / `hotpath-profiling`: refusals 3 / 4, exit 2 |
| 4 | `grid --json --machine i7-12700 --out .tmp/base.json`, then `--check --baseline`: all ten `ok`, PASS, exit 0 |
| 5 | `heavy_sgr` baseline x 3: `heavy_sgr` `SLOWER` (0.32), FAIL line, exit 1 |
| 6, 7 | hotpath table names `Terminal::feed`, `Parser::advance`, `Screen::scroll_up`; with `render --fixture dense_cells --frames 100` also `Terminal::snapshot_update` |
| 8 | debug `parser --json --machine M`: `refusing --json: ...`, exit 1 |
| 9 | release dry run: see below |

## Release dry run

Clone of HEAD (`a7d2271`, without the phase 12 work) under `.tmp/release-dry-run` instead
of `.claude/worktrees/`, deleted afterwards; `git diff HEAD -- scripts/release.sh` in the main tree
is empty (unchanged).

- First attempt: every step passed (fmt, clippy, test, `all --mib 32 --json`, `chart`) and the
  script stopped at `git commit` with `unable to auto-detect email address`: a fresh clone does
  not inherit the repository's local `user.name` / `user.email`. Environment, not the script.
- Second attempt, after `git config user.name` / `user.email` inside the clone and
  `git reset --hard` there (the aborted run had staged `perf/`): exit 0, last line
  `next: git push origin main "v0.1.0"`; the clone holds commit `release: v0.1.0`, tag `v0.1.0`,
  `perf/history/v0.1.0.json` and `perf/chart.svg`, so the record parsed and `chart` ran.
- `vt-bench grid --check --baseline <clone>/perf/history/v0.1.0.json` from the main tree
  (unpinned, so the figures are the noisy ones): all ten `ok`, ratios 0.97-1.03,
  `PASS: 10 of 10 fixtures at or above 0.50 x baseline`, exit 0.
- Shared `CARGO_TARGET_DIR` caveat: the clone's release build overwrites
  `target/release/vt-bench.exe`; rebuild in the main tree before using it again.
