# `vt-bench grid --check`: the regression trip-wire

```text
vt-bench grid --check [--baseline FILE] [--tolerance R] [--mib N]
```

Measures tier 2 like `grid` and compares each fixture's MiB/s with a baseline record. A
factor band, not a percentage: it catches structural regressions (a clone per cell, a linear
scan in a per-byte loop, a lost fast path), not drift. Run by hand on the baseline machine.

## Flags

| Flag | Default | Rule |
|---|---|---|
| `--baseline FILE` | newest record in `perf/history/` | any record readable by `perf_history::parse_release` (a `grid --json` output qualifies) |
| `--tolerance R` | `0.5` | number with `0 < R <= 1`; else `--tolerance needs a number in (0, 1], got '<v>'` |
| `--mib N` | the baseline's `mib_per_fixture` (32 if absent/0) | `1..=4096`, same parser as the measure commands |

Any other flag (`--json`, `--out`, `--machine`, `--frames`, `--fixture`) ->
`'grid --check' takes only --baseline, --tolerance and --mib`.

**Default baseline (D1).** `concat!(env!("CARGO_MANIFEST_DIR"), "/../../perf/history")`, so the
result does not depend on the working directory; `load_history` already sorts by
`ReleaseVersion`, the newest is its last element (`v0.10.0` beats `v0.9.0`). Why not a separate
committed baseline file: every release record is already a full tier-2 run on the baseline
machine, captured by `scripts/release.sh` with the same command, and perf/README's rule (never
edit; replace only in a commit that says why) is exactly the refresh discipline a baseline needs.
A second file would be a second copy of the same figures that drifts. Between releases,
`--baseline FILE` covers ad-hoc comparisons (`grid --json --machine T --out FILE` first).

## Refusals: no verdict, exit 2, checked in this order before anything is measured

| # | Condition | Message after `vt-bench: ` |
|---|---|---|
| 1 | bad flag or value | per table above |
| 2 | `cfg!(debug_assertions)` | `--check needs a release build (cargo run --release)` |
| 3 | bench feature `vt-paranoid` | `--check refuses under vt-paranoid: the whole-history walk dominates the timing` |
| 4 | bench feature `hotpath-profiling` | `--check refuses under hotpath-profiling: measurement sites change the timing` |
| 5 | default dir missing or no `*.json` | `no perf record in perf/history; scripts/release.sh writes the first, or pass --baseline FILE` |
| 6 | `--baseline` unreadable / invalid | `{FILE}: cannot read file` / the `parse_release` error (names the file) |
| 7 | baseline `columns`x`lines` non-zero and != 160x45 | `{src}: baseline geometry {c}x{l}, this build measures 160x45` |
| 8 | fixture sets differ | ``fixture sets differ: not in baseline: `a`, `b`; not measured: `c` `` (empty side printed as `-`) |
| 9 | baseline MiB/s not finite or <= 0 | ``{src}: baseline MiB/s for `x` is not a positive number`` |

Rows 2-4 come from one pure function over a `BuildFlavour { debug, paranoid, profiling }`
filled with `cfg!(...)` in one place (testable with any combination). The same function makes
`--json` on any measure command fail with `refusing --json: <reason>` (exit 1; checked in
`measure_command` after flag parsing, not in `parse_measure_options`): a skewed build must
never become a history record. Rows 3-4 read the **bench crate's** passthrough features
(hotpath.md); `--features oneterm-vt/vt-paranoid` on the command line bypasses them, which
`docs/user/performance.md` says in one line.

A fixture missing on either side is a refusal, never a skip: a renamed fixture must not drop
out of the guard unnoticed. Refresh the baseline in the commit that renames it.

## Verdict

- Measure: `measure_tier(mib, all fixtures, grid_pass)`; progress line on stderr as for `grid`.
- A fixture is `SLOWER` when `now < tolerance * baseline`; exactly at the band passes.
- Rows follow `FIXTURES` order. Exit **0** when no row is `SLOWER`, **1** otherwise.

Exact stdout (`MiB/s` `{:.1}`, ratio `{:.2}`, tolerance `{:.2}`):

```text
# vt-bench grid --check

Baseline: perf/history/v0.1.0.json (v0.1.0, 2026-11-30)
Baseline machine: i7-12700 12C/20T, Windows 11 26200, rustc 1.96.0
Run: 160x45, 32 MiB per fixture, median of 5 cycles after 1 warm-up.
A fixture fails below 0.50 x its baseline MiB/s; exactly 0.50 passes.

| Fixture | baseline MiB/s | now MiB/s | ratio | verdict |
|---|---:|---:|---:|---|
| `plain_ascii` | 82.9 | 81.0 | 0.98 | ok |
| `heavy_sgr` | 204.1 | 90.2 | 0.44 | SLOWER |

FAIL: 1 of 10 fixtures below 0.50 x baseline: `heavy_sgr`
```

Last line on success: `PASS: 10 of 10 fixtures at or above 0.50 x baseline`. The baseline path
prints as `perf/history/<file name>` for the default and verbatim for `--baseline`. An empty
`date` prints `(v0.1.0)` without the comma part.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | PASS: every fixture at or above the band |
| 1 | FAIL: at least one fixture `SLOWER` (the table is still printed in full) |
| 2 | no verdict: a refusal or a flag error (matches the CLI's exit 2 for usage errors) |

## Never in CI

- The baseline is one machine's; a CI runner is a different machine, so its ratio means nothing.
- Shared runners vary run to run by more than the band (our own desktop spreads reached 62% in
  0002's tier-1 notes); a flaky gate trains people to re-run it, which is worse than no gate.
- A full check is 60 passes of 32 MiB (minutes); CI time is better spent on tests.
- CI does compile both hotpath halves (hotpath.md), so the check's code never rots.

## Code (`crates/vt-bench`)

| Item | Where |
|---|---|
| `check_command(args) -> Result<CheckReport>`, flag parsing, baseline resolution, refusals 5-9 | `regression_check.rs` (new) |
| `BuildFlavour`, `skew_reason(BuildFlavour) -> Option<&'static str>` | `regression_check.rs` |
| `compare(baseline: &[Throughput], now: &[Throughput], tolerance) -> Vec<CheckRow>`; `render_check(..) -> String` | `regression_check.rs`, pure |
| routing `grid` + `--check`, exit mapping (`CheckReport::passed` -> 0/1, `Err` -> 2) | `main.rs::run` |
| USAGE line `grid --check [--baseline FILE] [--tolerance R] [--mib N]` | `main.rs` |
