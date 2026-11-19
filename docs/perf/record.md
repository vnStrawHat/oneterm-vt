# The perf records

A release writes two files per platform ([README](README.md)).

## The run of one release: `perf/history/<platform>/v<version>.json`

Written by `vt-bench all --json --machine TEXT --out FILE`: every tier, one build.

```json
{
  "engine": "oneterm-vt",
  "version": "0.1.0",
  "date": "2026-11-30",
  "platform": "windows",
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
| `version` | workspace version, `N.N.N` |
| `date` | UTC `YYYY-MM-DD` of the run |
| `platform` | `std::env::consts::OS` of the build that ran (`linux`, `windows`, `macos`); names the folder |
| `machine` | free text from `--machine`; the release runner's description |
| `columns`, `lines` | grid geometry for tiers 1-3 |
| `mib_per_fixture`, `runs` | input size per fixture; cycles per fixture (median reported) |
| `mib_per_second` | median throughput; `ns_per_byte` is the same figure inverted |
| `spread_percent` | (slowest - fastest) / median x 100; a difference smaller than this is noise |
| `us_per_frame` | tier 3: one `snapshot_update` per simulated frame |
| `cells` | tier 3: cells copied per frame (rows copied x columns, median cycle), not the viewport area |
| `grow_us`, `shrink_us` | tier 4: resize 80x24 to 100x40 and back at a scrollback depth |
| `heap_bytes*` | tier 5: live heap after filling 10 000 scrollback rows of one kind: `plain`, `unicode`, `styled`, `mixed` |

Nothing reads these files back. Their MiB/s hold for that runner in that minute only.

## The last releases in turns: `perf/in-turns/<platform>/v<version>.json`

Written by `vt-bench grid --check --against ... --machine TEXT --out FILE`
([performance across releases](../user/performance-releases.md)): tier 2 of several builds,
measured side by side in one job. `vt-bench chart --in-turns FILE` draws it.

```json
{
  "engine": "oneterm-vt",
  "date": "2026-11-30",
  "platform": "linux",
  "machine": "GitHub ubuntu-latest, Ubuntu 24.04, 4 vCPU AMD EPYC 7763 64-Core Processor, rustc 1.96.0",
  "columns": 160,
  "lines": 45,
  "mib_per_fixture": 32,
  "rounds": 5,
  "runs": 5,
  "builds": [
    { "version": "0.4.2", "tier2_grid": [{ "fixture": "plain_ascii", "mib_per_second": 0.0, "rounds": [0.0, 0.0, 0.0, 0.0, 0.0] }] },
    { "version": "0.4.3", "tier2_grid": [{ "fixture": "plain_ascii", "mib_per_second": 0.0, "rounds": [0.0, 0.0, 0.0, 0.0, 0.0] }] }
  ]
}
```

| Field | Meaning |
|---|---|
| `rounds` (top) | how often every build ran every fixture |
| `runs` | timed cycles inside one run; a run reports their median |
| `builds` | in the order given, oldest first; the build that measured, the release itself, is last |
| `builds[].version` | the version that build's own report names |
| `mib_per_second` | the fastest round: the machine can only slow a round down |
| `rounds` (per fixture) | every round as measured, in order; how far apart they are says how quiet the job was |

A fixture a build lacks, or generates another stream for, has no entry in that build.
