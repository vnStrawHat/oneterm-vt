# Output: results JSON, markdown, chart, machine record

## Paths

| Path | What | Committed |
|---|---|---|
| `perf/compare/<machine>/<YYYY-MM-DD>.json` | full run (schema below) | yes |
| `perf/compare/<machine>/<YYYY-MM-DD>.md` | methodology block + tables (below) | yes |
| `perf/compare/<machine>/<YYYY-MM-DD>.svg` | chart | with the JSON |
| `.tmp/compare/` | corpus, verify dumps, raw adapter outputs, Zig local cache | never |

`<machine>` is the `--machine` slug (`[a-z0-9-]+`, e.g. `i7-12700-win11`); a second run on the
same day gets `-2`. Same never-edit rule as `perf/history` (docs/perf/README.md). Results
are not quoted in the root README or `docs/user/`.

## Results JSON (`schema = 1`)

```json
{
  "schema": 1, "date": "2026-11-02", "unpinned": false,
  "machine": { "name": "i7-12700-win11", "cpu": "12th Gen Intel(R) Core(TM) i7-12700",
    "logical_cpus": 20, "ram_gib": 32, "os": "Windows 11 Enterprise 10.0.26300",
    "power_scheme_before": "Balanced 381b4222-...", "power_scheme_during": "High performance 8c5e7fda-...",
    "on_ac": true, "pinned_cpu": 4, "priority": "high" },
  "toolchains": { "rustc": "1.96.0", "zig": "0.17.0", "node": "v22.20.0", "msvc": "17.14" },
  "corpus": { "version": 1, "mib": 32, "lock_sha256": "..." },
  "knobs": { "cols": 160, "rows": 45, "scrollback": 10000, "chunk": 65536,
             "warmup": 1, "passes": 5, "repetitions": 3, "checkpoints": 4 },
  "engines": [ { "name": "ghostty", "version": "1.4.1-dev", "source": "ghostty-org/ghostty@b2c78c9",
                 "language": "zig", "build": "ReleaseFast, cpu=baseline, static",
                 "capabilities": ["graphemes", "hyperlinks"], "labels": [] } ],
  "grid": [ { "engine": "ghostty", "fixture": "plain_ascii", "status": "ok",
              "mib_per_second": 0.0, "ns_per_byte": 0.0, "spread_percent": 0.0,
              "ratio_to_oneterm": 0.0, "samples_ns": [], "steady_mib": 0.0, "peak_mib": 0.0,
              "first_difference": null } ],
  "summary": { "status_counts": { "ghostty": { "ok": 0, "accepted": 0, "diverged": 0,
                 "unverified": 0, "unsupported": 0, "failed": 0 } },
               "intersection": ["plain_ascii"], "geomean_ratio_to_oneterm": { "ghostty": 0.0 } },
  "wall_time_s": 0
}
```

`unsupported` rows carry no numbers (`null`); `failed` rows carry `"error"` (first stderr
line, 200 chars). `labels`: adapter notes shown beside the engine name (`+ring` for libvterm,
`+utf8 decode` for avt, `JIT/GC on pinned core` for xterm).

## Machine capture (automatic, by the driver)

| Field | Source |
|---|---|
| `cpu` | CPUID brand string (`core::arch::x86_64::__cpuid` leaves `0x8000_0002..4`) |
| `logical_cpus` | `std::thread::available_parallelism` |
| `ram_gib` | `GlobalMemoryStatusEx` (Windows), `/proc/meminfo` (Linux) |
| `os` | `cmd /c ver` (Windows), `/etc/os-release` + `uname -r` (Linux) |
| `power_scheme_before`, `power_scheme_during`, `on_ac` | `powercfg /getactivescheme` before and after the switch (criteria.md), `GetSystemPowerStatus`; warn if not on AC |
| toolchains | `rustc -V`, `zig version`, `node -v`, `vswhere -latest -property catalog_productDisplayVersion` |
| engine versions | each adapter's `info` |

## Markdown (`.md`)

1. Header: date, machine line (both power schemes), toolchains, knobs, corpus version,
   `unpinned` warning if set; then a fixed methodology block (layer, statistics, gate,
   status legend; about 15 lines from a driver template) so a committed run stands alone.
2. Engines table: name, version/pin, language, build, capabilities.
3. **Grid table**: one row per fixture (corpus order), one column per engine, cell = MiB/s
   with `≈` (tie with the fastest), `†` (accepted), `≠` (diverged), `?` (unverified, ungated
   fixture), `n/a`, `fail`; the fastest verified cell in bold; a `spread` row-max column.
   Footer: per engine the status counts; then one geomean of `ratio_to_oneterm` per engine over
   the intersection (criteria.md "Summary"), with the intersection's fixtures listed.
4. Memory table (`steady_mib` per engine per fixture, `peak_mib` max), marked `?`.
5. Appendix: every `diverged`/`failed` pair with `first_difference` or `error`.

## Chart (`.svg`, no new dependency; hand-written like `vt-bench chart`)

- One panel per fixture class (core, capability): horizontal bars, one group per fixture,
  one bar per engine in the fixed engine order and colour; log-scale MiB/s axis (the probe
  spans 6-1100 MiB/s).
- Only `ok` and `accepted` bars are drawn; `accepted` bars hatched; missing bars leave a gap
  labelled `≠`, `?`, `n/a` or `fail`.
- Title carries machine, date, corpus version; legend lists engine versions.
