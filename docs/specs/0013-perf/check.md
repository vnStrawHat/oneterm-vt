# `vt-bench grid --check`: builds in turns

```text
vt-bench grid --check --against BINARY [--against BINARY ...] [--tolerance R] [--mib N]
    [--rounds N] [--machine TEXT --out FILE]
```

Runs tier 2 of this build and of other builds of `vt-bench` in turns on one machine. It is the
trip-wire of a release (this build against the last build given) and, with `--out`, the perf
history of that release: the figures of every build, which the chart draws.

Why builds in turns and a fixture at a time, with the figures of each design that was tried,
and what the band and the three rounds rest on: [check-evidence.md](check-evidence.md).

## Flags

| Flag | Default | Rule |
|---|---|---|
| `--against BINARY` | at least one | repeatable, order kept; else `'grid --check' needs --against BINARY: a vt-bench built from a release to compare with` |
| `--tolerance R` | `0.75` | number with `0 < R <= 1`; else `--tolerance needs a number in (0, 1], got '<v>'` |
| `--mib N` | 32 | `1..=4096`, same parser as the measure commands |
| `--rounds N` | 3 | `1..=99`; else `--rounds needs a whole number 1..=99, got '<v>'` |
| `--machine TEXT` | none | free text for the record |
| `--out FILE` | none | writes the in-turns record (docs/perf/record.md); without `--machine`: `--out needs --machine: the record names where it was measured` |

Any other flag -> `'grid --check' takes only --against, --tolerance, --mib, --rounds, --machine and --out`.

## Method

1. Builds = every `--against` in order, then this binary (`current_exe`).
2. Shared streams: every build runs `fixtures --mib 1 --out <scratch>/<index>`. A build runs a
   fixture of this build only when its `<name>.vt` is byte for byte this build's: a missing or
   different stream is not measured (a figure of another stream says nothing about the engine).
   The scratch folder (under the temp dir) is removed.
3. Per round, per fixture, per build that shares it: `BINARY grid --fixture NAME --mib N`, a
   process of its own. The order of the builds shifts by one each round. The row
   `` | `NAME` | MiB/s | .. `` gives the figure, `# vt-bench: oneterm-vt X.Y.Z` the version.
4. Per build and fixture every round is kept, and the best is the figure: a round can only be
   slowed by the machine.
5. A fixture is `SLOWER` when `this < R * last build given`; exactly at the band passes; `not
   judged` when that build did not run it. One only another build has is not a row.

## No verdict (exit 2), after `vt-bench: `

| Condition | Message |
|---|---|
| bad flag or value | per the table above |
| `cfg!(debug_assertions)` | `--check needs a release build (cargo run --release)` |
| bench feature `vt-paranoid` | `--check refuses under vt-paranoid: the whole-history walk dominates the timing` |
| bench feature `hotpath-profiling` | `--check refuses under hotpath-profiling: measurement sites change the timing` |
| a binary does not start | `{BINARY}: cannot run it` (and the OS error) |
| it exits non-zero | `{BINARY}: 'fixtures --mib 1' failed:` or `{BINARY}: 'grid --fixture NAME --mib N' failed:`, then its stderr |
| its report has no row for the fixture | `{BINARY}: 'grid --fixture NAME --mib N' printed no row for it` |
| the last build given shares no fixture | `{BINARY}: no fixture in common with this build` |

The build rows come from `skew_reason(BuildFlavour)`, which also makes `--json` on a measure
command fail with `refusing --json: <reason>`: a skewed build must never become a record.

## Output and exit codes

```text
# vt-bench grid --check

This build: oneterm-vt 0.4.3
Other builds, in the order given: 0.4.1, 0.4.2
Run: 160x45, 32 MiB per fixture, 3 rounds; in a round every build runs a fixture before any runs the next one. A run is the median of 5 cycles after 1 warm-up; a figure is the best round, in MiB/s.
A fixture fails below 0.75 x the figure of 0.4.2, the last build given; exactly 0.75 passes.

| Fixture | 0.4.1 | 0.4.2 | this build | this / 0.4.2 | verdict |
|---|---:|---:|---:|---:|---|
| `plain_ascii` | 880.0 | 900.0 | 910.0 | 1.01 | ok |
| `brand_new` | - | - | 77.0 | - | not judged |

`-`: the build has no such fixture or generates another stream for it, so it did not run it.

PASS: 17 of 17 fixtures at or above 0.75 x 0.4.2
FAIL: 1 of 17 fixtures below 0.75 x 0.4.2: `heavy_sgr`
```

A build is named by the version it reported, or by its file name when it ran nothing. `0` pass,
`1` at least one fixture `SLOWER` (the table is printed in full, and `--out` is written), `2` no
verdict.

## Where it runs

The Release workflow, in every bench job (0014 release.md): the last five release tags are built
with the job's toolchain and passed oldest first; `throughput_tolerance` is the input for `R`.
Never on pushes and pull requests. By hand: build the tags you compare with, pass each
`vt-bench`.

## The chart

`vt-bench chart --in-turns FILE --out FILE`: one panel, tier 2 in MiB/s, one line per fixture,
one point per build that ran it (title text of a point: best round and the slowest). The footer
names the date, the rounds and the machine. It replaces the chart of one record per release
(`--history DIR`, two panels): `perf_history` no longer reads `perf/history` back.

## Tests

`regression_check_tests.rs`: flavour refusals; defaults and every flag, `--against` repeated;
each refusal text; a debug build gives no verdict; a binary that cannot run is named (grid and
streams); a report gives the version and one rate per fixture, and what
`throughput::render_table` prints is what is read; the turn order; best round beside every
round; a build's label; held against the last build given only, the band edge, not judged; a
slowdown of every fixture fails; the exact report. `perf_history`: the in-turns record round
trips in schema order, a broken file is named. `chart`: lines and points, a fixture a build
lacks, escaping, title and footer, the legend fits.
