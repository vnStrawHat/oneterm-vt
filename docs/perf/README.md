# Performance history

Every release stores one benchmark run, and a chart compares releases.

| Path | What |
|---|---|
| `perf/history/v<version>.json` | the run captured on the Release workflow's runner just before tagging |
| `perf/chart.svg` | tier-2 MiB/s per fixture across all releases (`vt-bench chart`) |

Rules:

- Never edit a history file. A wrong record is replaced only in a commit that says why.
- Records come from the release runner (`ubuntu-latest`); `machine` names it:
  `GitHub ubuntu-latest, <os> <version>, <n> vCPU <cpu model>, rustc <version>`. The chart warns
  when strings differ (other hardware or compiler); compare such releases with care.
- Hosted runners are noisy. The Release workflow runs `grid --check` against the previous record
  (band `0.5`): a trip-wire for structural regressions, not a quality verdict.
- The quality verdict is the tier-2 target table below, measured by hand on a quiet machine.
  CI on pushes and pull requests never benchmarks.

## Commands

```bash
# capture (the Release workflow does this through release.sh)
cargo run -p oneterm-vt-bench --release -- all --mib 32 --json --machine "<runner, os, cores, cpu, rustc>" --out perf/history/v0.1.0.json
# chart
cargo run -p oneterm-vt-bench --release -- chart --history perf/history --out perf/chart.svg
# compare with a record (newest perf/history record by default)
cargo run -p oneterm-vt-bench --release -- grid --check
# where does heavy_sgr spend its time? (hotpath-profiling sites; one fixture per report)
cargo run -p oneterm-vt-bench --release --features hotpath-profiling -- grid --fixture heavy_sgr --mib 32
# per frame (tier 3 reaches snapshot_update)
cargo run -p oneterm-vt-bench --release --features hotpath-profiling -- render --fixture dense_cells
```

## Record schema

```json
{
  "engine": "oneterm-vt",
  "version": "0.1.0",
  "date": "2026-11-30",
  "machine": "i7-12700 12C/20T, Windows 11 26200, rustc 1.96.0",
  "columns": 160,
  "lines": 45,
  "mib_per_fixture": 32,
  "runs": 5,
  "tiers": {
    "tier1_parser": [{ "fixture": "plain_ascii", "mib_per_second": 0.0, "ns_per_byte": 0.0, "spread_percent": 0.0 }],
    "tier2_grid":   [{ "fixture": "plain_ascii", "mib_per_second": 0.0, "ns_per_byte": 0.0, "spread_percent": 0.0 }],
    "tier3_render": [{ "fixture": "plain_ascii", "us_per_frame": 0.0, "cells": 0 }],
    "tier4_resize": [{ "scrollback_rows": 10000, "grow_us": 0.0, "shrink_us": 0.0 }],
    "tier5_memory": [{ "content": "plain", "heap_bytes": 0, "heap_bytes_per_row": 0.0 }]
  }
}
```

| Field | Meaning |
|---|---|
| `version` | workspace version, `N.N.N`; the chart sorts by it |
| `date` | UTC `YYYY-MM-DD` of the run |
| `machine` | free text from `--machine`; required; the release runner's description |
| `columns`, `lines` | grid geometry for tiers 1-3 |
| `mib_per_fixture`, `runs` | input size per fixture; cycles per fixture (median reported) |
| `mib_per_second` | median throughput; `ns_per_byte` is the same figure inverted |
| `spread_percent` | (slowest - fastest) / median x 100; a difference smaller than this is noise |
| `us_per_frame` | tier 3: one `snapshot_update` per simulated frame |
| `cells` | tier 3: cells copied per frame (rows copied x columns, median cycle), not the viewport area |
| `grow_us`, `shrink_us` | tier 4: resize 80x24 to 100x40 and back at a scrollback depth |
| `heap_bytes*` | tier 5: live heap after filling 10 000 scrollback rows of one kind: `plain`, `unicode`, `styled`, `mixed` |

The chart reads `version`, `machine` and `tiers.tier2_grid` only.

## Fixtures (tiers 1-3)

`plain_ascii`, `long_lines` (wrap every line), `heavy_sgr` (24-bit SGR per char), `tui_redraw`
(CUP + short text), `scroll_region` (DECSTBM then line feeds), `cjk_wide`, `dense_cells`
(256-colour SGR per cell), `scrolling` (grid-width lines), `sixel` (small images),
`osc_9_7` (an `OSC 9;7` status message before every line). Deterministic generators; no files.

## Tier-2 targets (phase 12 acceptance)

Machine: Intel Core i7-12700 (12C/20T), Windows 11, rustc 1.96.0, release profile,
160x45, 32 MiB per fixture, median of 5. A fixture passes at >= 0.90 x its target.

| Fixture | MiB/s | Fixture | MiB/s |
|---|---:|---|---:|
| plain_ascii | 75 | cjk_wide | 115 |
| long_lines | 81 | dense_cells | 196 |
| heavy_sgr | 227 | scrolling | 80 |
| tui_redraw | 126 | sixel | 47 |
| scroll_region | 61 | osc_9_7 | 102 |

## Waivers

A fixture below its pass line, accepted by the owner after a profile. Re-examined each release.

| Fixture | Target | Measured | Ratio | Why (profile summary) | Approved |
|---|---:|---:|---:|---|---|
| - | | | | none | |

Tiers 3-5 have no target: recorded per release, not targeted; compare releases on one machine.
