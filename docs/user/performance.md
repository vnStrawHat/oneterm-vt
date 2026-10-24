# Performance

How fast the engine is, how that is measured, and how to check a change did not slow it down.
The tool is `vt-bench` (`crates/vt-bench`, a developer binary that is not part of the library).

## What is measured

| Tier | Command | Isolates | Unit |
|---|---|---|---|
| 1 | `parser` | `parser::Parser` alone, output dropped | MiB/s |
| 2 | `grid` | `Terminal::feed`: parse and apply to the grid, in 64 KiB chunks | MiB/s |
| 3 | `render` | feed plus one `Terminal::snapshot_update` per simulated frame | us/frame, cells/frame |
| 4 | `resize` | resize 80x24 to 100x40 and back at 0, 10 000 and 100 000 scrollback rows | us |
| 5 | `rss` | live heap after 10 000 scrollback rows of plain, unicode, styled and mixed text | bytes/row |

Tier 2 is the headline: it is what every byte of terminal output costs an embedder. Tiers 1-3
run ten deterministic streams at 160x45: `plain_ascii`, `long_lines`, `heavy_sgr`, `tui_redraw`,
`scroll_region`, `cjk_wide`, `dense_cells`, `scrolling`, `sixel`, `osc_9_7`.

## What is not measured

No renderer, no pseudo-console, no threads or channels, no lock: one in-memory buffer fed from
one thread. One machine, one build. A number here is the engine's own cost, not the cost of a
whole terminal application.

## The numbers

Tier 2, release build, median of three runs of `grid --mib 32`, each the median of five cycles;
every figure was taken pinned to one performance core at high priority (see Reproduce).
Intel Core i7-12700 (12C/20T), Windows 11, rustc 1.96.0, 2026-10-24. `spread` is
(slowest - fastest) / median over the five cycles of the noisiest run: a difference smaller than
it is noise.

| Fixture | MiB/s | spread | Fixture | MiB/s | spread |
|---|---:|---:|---|---:|---:|
| `plain_ascii` | 154.5 | 13% | `cjk_wide` | 142.3 | 12% |
| `long_lines` | 161.5 | 11% | `dense_cells` | 187.5 | 11% |
| `heavy_sgr` | 262.8 | 7% | `scrolling` | 161.9 | 14% |
| `tui_redraw` | 199.3 | 10% | `sixel` | 42.7 | 14% |
| `scroll_region` | 90.3 | 10% | `osc_9_7` | 160.3 | 11% |

Each release's own run is `perf/history/v<version>.json`; `perf/chart.svg` compares releases.

## Reproduce

```bash
cargo run -p oneterm-vt-bench --release -- grid --mib 32
cargo run -p oneterm-vt-bench --release -- all --mib 32 --json --machine "<runner, os, cores, cpu, rustc>" --out run.json
```

- Always `--release`; debug numbers say nothing about the shipped code.
- Quiet machine: close builds, editors and sync clients. On a hybrid CPU, pin the process to one
  performance core (on Windows `start /b /wait /high /affinity 10 vt-bench.exe ...`); core
  migration alone can double the spread.
- Never pipe a generator into the tool; fixtures are generated in memory, outside the timer.
- `--json` refuses a debug build, `vt-paranoid` and `hotpath-profiling`: a skewed build must
  never become a history record.

## History and chart

The Release workflow stores one tier 1-5 run per release, made on its hosted Linux runner, in
`perf/history/v<version>.json` and redraws `perf/chart.svg` (tier-2 MiB/s per fixture across
releases). Compare runs from one machine only; the record's `machine` names the runner. The record schema is in [docs/perf/README.md](../perf/README.md).

## The trip-wire

```bash
cargo run -p oneterm-vt-bench --release -- grid --check
```

Measures tier 2 and compares each fixture with a baseline record. It catches structural
regressions (a clone per cell, a linear scan in a per-byte loop, a lost fast path), not drift.

- Baseline: the newest `perf/history/v*.json`, or `--baseline FILE` (any `grid --json` output).
- `--tolerance R` (default `0.5`): a fixture fails below `R` x its baseline, exactly `R` passes.
  `--mib N` defaults to the baseline's input size.
- Exit codes: `0` pass, `1` at least one fixture `SLOWER`, `2` no verdict.
- No verdict (exit 2): bad flag; debug, `vt-paranoid` or `hotpath-profiling` build; no history
  and no `--baseline`; unreadable baseline; baseline geometry not 160x45; fixture sets differ;
  a baseline figure that is not a positive number.
- The build refusals read `vt-bench`'s own features; `--features oneterm-vt/vt-paranoid` on the
  command line bypasses them, so do not.
- The Release workflow runs it against the previous release record before pushing; the wide
  band absorbs runner noise, so it catches only structural regressions. The numbers table above
  (a quiet machine) is the quality verdict.

## Profiling

For contributors. The `hotpath-profiling` feature adds five `hotpath::measure` sites:
`Terminal::feed`, `Terminal::snapshot_update`, `Parser::advance`, `Screen::scroll_up` and
`Screen::scroll_down`. On its own the feature costs nothing; the sites measure only in a binary
that also enables `hotpath/hotpath`, which `vt-bench`'s own feature does.

```bash
cargo run -p oneterm-vt-bench --release --features hotpath-profiling -- grid --fixture heavy_sgr --mib 32
cargo run -p oneterm-vt-bench --release --features hotpath-profiling -- render --fixture dense_cells
```

`--fixture NAME` (on `parser`, `grid`, `render`) runs one stream, so the report belongs to it.
`feed` contains `advance`, which contains the scrolls: read the report as a tree.

## Not a promise

Performance is outside the semver promise ([versioning.md](versioning.md)): a later version may
be slower without a major bump. These numbers are evidence about one build on one machine on
one day.
