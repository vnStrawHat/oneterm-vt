# `vt-bench` CLI and `chart`

Hand-parsed arguments (no `clap`): the flag set is small and fixed. Decision for phase 0:
**`chart` is fully implemented**; every measuring command is a stub that errors. `chart` needs
nothing from the engine and `scripts/release.sh` depends on it.

## Usage text (constant `USAGE`, printed verbatim)

```text
vt-bench <command> [flags]

Measure (lands phase by phase; see the roadmap):
  all | parser | grid | render | resize | rss | fixtures
      [--mib N] [--frames N] [--json] [--out FILE] [--machine TEXT]
  grid --check [--baseline FILE] [--tolerance R]

Report:
  chart --history DIR --out FILE   SVG of tier-2 MiB/s per release, one line per fixture

  help | --help | -h               print this text
```

## Behaviour in phase 0

| Invocation | Output | Exit |
|---|---|---|
| `help`, `--help`, `-h` (first argument) | `USAGE` on stdout | 0 |
| no arguments, unknown command | `USAGE` on stderr | 2 |
| `all`, `parser`, `grid`, `render`, `resize`, `rss`, `fixtures` | `vt-bench: '<cmd>' is not implemented yet` on stderr | 1 |
| `chart` missing `--history` or `--out`, unknown flag, flag without value | message naming the flag | 1 |
| `chart --history DIR --out FILE` | writes the SVG, prints `wrote FILE` | 0 |

`main` returns `std::process::ExitCode`; inner functions return `anyhow::Result` and errors print as
`vt-bench: {error:#}`. Printing lives in `main.rs` only; put
`#[allow(clippy::print_stdout)]` on the one function that prints, with a "why" comment, as the
template's `main.rs` did.

## `perf_history.rs`: reading `perf/history/*.json`

Schema: [docs/perf/README.md](../../perf/README.md). Chart reads only the fields below; serde
ignores the rest. Later phases extend these same structs (add `Serialize` and the other tiers), so the
schema keeps one Rust home.

```rust
#[derive(Deserialize)] pub(crate) struct PerfRecord { version: String, machine: String, tiers: Tiers }
#[derive(Deserialize)] pub(crate) struct Tiers { tier2_grid: Vec<Throughput> }
#[derive(Deserialize)] pub(crate) struct Throughput { fixture: String, mib_per_second: f64, spread_percent: f64 }

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct ReleaseVersion { major: u64, minor: u64, patch: u64 } // field order = semver order
impl FromStr for ReleaseVersion  // exactly `N.N.N`, digits only; anything else is an error
impl Display for ReleaseVersion  // `N.N.N`

pub(crate) struct Release { version: ReleaseVersion, record: PerfRecord }

pub(crate) fn parse_release(source: &str, json: &str) -> anyhow::Result<Release>;
pub(crate) fn sort_releases(releases: Vec<Release>) -> anyhow::Result<Vec<Release>>; // ascending; duplicate version = error
pub(crate) fn load_history(dir: &Path) -> anyhow::Result<Vec<Release>>;              // *.json, not recursive
```

- `source` is the file path, used in every error (`"perf/history/v0.2.0.json: version '0.2' is not N.N.N"`).
- `load_history` errors: directory unreadable, no `*.json` file, any file unreadable or invalid.
  It is thin I/O; logic sits in the two pure functions so tests need no files.

## `chart.rs`: hand-written SVG

```rust
pub(crate) fn render_svg(releases: &[Release]) -> String; // releases already sorted
```

| Element | Rule |
|---|---|
| Canvas | `960 x 540`, `viewBox` set, white background `<rect>`, font `sans-serif` 12px |
| Plot area | margins left 70, right 190 (legend), top 50, bottom 60 |
| Title | `oneterm-vt tier 2 throughput (MiB/s, higher is better)` |
| X axis | one evenly spaced category per release, label `vN.N.N`; a single release sits centred |
| Y axis | 0 to `nice_ceiling(max)`, a gridline + label per step, label `MiB/s` |
| Series | one per fixture, in first-seen order across releases; colour from a fixed 10-entry palette, cycled |
| Lines | a `<polyline>` through the points a fixture has (missing releases are skipped); none if it has < 2 points |
| Points | a `<circle r="3">` per point with `<title>{fixture} v{version}: {mib:.1} MiB/s (spread {spread:.1}%)</title>` |
| Legend | right margin, one coloured swatch + name per fixture |
| Machine note | if the releases name more than one distinct `machine`, a footer line: `warning: N machines in this history; compare runs from one machine only` |

Helpers (private): `nice_ceiling(max: f64) -> (f64 /*top*/, f64 /*step*/)` targets about five
steps with step in `{1, 2, 2.5, 5, 10} x 10^k`; `max <= 0` gives `(1.0, 0.2)`.
`escape_xml(&str) -> String` escapes `& < > " '`. Numbers print with `{:.1}`. Output is
deterministic: no timestamps, no randomness, no hash-map iteration order.

`main.rs` creates the parent directory of `--out` if missing.

## Tests (unit, `#[cfg(test)] mod tests` beside the code)

| Module | Test |
|---|---|
| perf_history | `"0.1.0"` parses; `"0.1"`, `"v0.1.0"`, `"0.1.0-rc.1"`, `"a.b.c"` are errors |
| perf_history | ordering: `0.9.0 < 0.10.0 < 1.0.0` |
| perf_history | `parse_release` on invalid JSON or missing `tiers.tier2_grid`: error text contains `source` |
| perf_history | `sort_releases` sorts out-of-order input; duplicate version is an error |
| chart | 2 releases x 2 fixtures: exactly 2 `<polyline`, 4 `<circle` |
| chart | 1 release: 0 `<polyline`, one `<circle` per fixture |
| chart | fixture name `a<b&c` appears only as `a&lt;b&amp;c` |
| chart | machine note present with 2 machines, absent with 1 |
| chart | `nice_ceiling`: `74.9 -> (80, 20)`, `226.5 -> (250, 50)`, `0.0 -> (1, 0.2)` |
| main | `USAGE` contains every command name listed above |
