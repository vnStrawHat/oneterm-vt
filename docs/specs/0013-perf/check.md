# `vt-bench grid --check`: the regression trip-wire

```text
vt-bench grid --check [--baseline FILE] [--tolerance R] [--mib N] [--machine TEXT]
```

Measures tier 2 like `grid` and compares each fixture's MiB/s with a baseline record. A
factor band, not a percentage: it catches structural regressions (a clone per cell, a linear
scan in a per-byte loop, a lost fast path), not drift. Run by hand on the baseline machine.

## Flags

| Flag | Default | Rule |
|---|---|---|
| `--baseline FILE` | newest record in `perf/history/<platform>/` (amended by 0014 platforms.md) | any record readable by `perf_history::parse_release` (a `grid --json` output qualifies) |
| `--tolerance R` | `0.5` | number with `0 < R <= 1`; else `--tolerance needs a number in (0, 1], got '<v>'` |
| `--mib N` | the baseline's `mib_per_fixture` (32 if absent/0) | `1..=4096`, same parser as the measure commands |
| `--machine TEXT` | none: the machine of the run is unknown | the text a record's `machine` would hold; see "Another machine" |

Any other flag (`--json`, `--out`, `--frames`, `--fixture`) ->
`'grid --check' takes only --baseline, --tolerance, --mib and --machine`.

**Default baseline (D1).** `concat!(env!("CARGO_MANIFEST_DIR"), "/../../perf/history")` joined
with `std::env::consts::OS` (`linux`, `windows`, `macos`), so the result does not depend on the
working directory and never crosses platforms; `load_history` already sorts by
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
| 5 | default dir missing or no `*.json` | `no perf record in perf/history/{os}; the Release workflow writes the first, or pass --baseline FILE` |
| 6 | `--baseline` unreadable / invalid | `{FILE}: cannot read file` (followed by the OS error) / the `parse_release` error (names the file) |
| 6a | baseline `platform` != `consts::OS` | `{src}: baseline platform {p}, this build runs on {os}` |
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

Baseline: perf/history/linux/v0.1.0.json (v0.1.0, 2026-11-30)
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
prints as `perf/history/<os>/<file name>` for the default and verbatim for `--baseline`. An empty
`date` prints `(v0.1.0)` without the comma part.

## Another machine (added with the Release workflow's `--machine`)

The first design assumed one baseline machine. The Release workflow runs the check on hosted
runners, and one runner label is several CPU models: a run on a slower model read every
fixture at 0.55 to 0.76 of the baseline and one at 0.48, with no change to that fixture's cost
relative to the others on that model. Two rules, both only when `--machine` is given:

- **Same machine** = equal text after dropping a trailing `, rustc <version>` (`hardware()`).
- **Default baseline**: the newest record of the same machine that passes refusals 6a-9; if
  there is none, the newest record. Figures of one machine are compared as they are, so a
  slowdown of every fixture is caught the next time a run lands on a machine the history knows.
- **Baseline of another machine** (default or `--baseline`): the band lies around the median
  of `now / baseline` over the judged fixtures instead of around 1. A fixture is `SLOWER` when
  `ratio < tolerance * median`; exactly at it passes; a fixture without a baseline is never
  slower. Known blind spot, printed in the docs: a slowdown shared by most fixtures moves the
  median and is not told from a slower machine.

Stdout then has one more line after `Baseline machine:` and another rule and verdict:

```text
Baseline machine: <baseline's>
This machine: <TEXT>
Run: 160x45, 32 MiB per fixture, median of 5 cycles after 1 warm-up.
The baseline is another machine's: fixtures are judged against the median ratio of this run, 0.59.
A fixture fails below 0.50 x that median (0.30); exactly at it passes.
...
PASS: 17 of 17 fixtures at or above 0.50 x the median ratio
FAIL: 1 of 17 fixtures below 0.50 x the median ratio: `heavy_sgr`
```

Without `--machine`, and on the baseline's machine, the output is the one above, byte for byte.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | PASS: every fixture at or above the band |
| 1 | FAIL: at least one fixture `SLOWER` (the table is still printed in full) |
| 2 | no verdict: a refusal or a flag error (matches the CLI's exit 2 for usage errors) |

## Never in CI

(On pushes and pull requests. The Release workflow does run it, with `--machine`: 0014.)

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
