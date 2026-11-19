# Performance history

Every release measures its last five predecessors beside itself on each platform, and one chart
per platform draws that. Platforms: `linux`, `windows`, `macos` (Rust's `std::env::consts::OS`).

| Path | What |
|---|---|
| `perf/in-turns/<platform>/v<version>.json` | tier 2 of the last five releases and of that release, every build run in turns in one job of the Release workflow |
| `perf/chart-<platform>.svg` | the newest in-turns record drawn: tier-2 MiB/s, one line per fixture across those builds (`vt-bench chart`) |
| `perf/history/<platform>/v<version>.json` | the release's own tier 1-5 run on that runner, just before tagging |
| `perf/compare/<machine>/<date>.{json,md,svg}` | cross-engine runs on one machine (not a release record); method in `compare/README.md` |

The GitHub release carries no copies: read them at the release tag. The root README embeds the charts.

Rules:

- Never edit a record. A wrong one is replaced only in a commit that says why.
- A figure in MiB/s holds for one machine in one minute: one runner label is several CPU models,
  and one model measured an unchanged engine a factor of three apart from run to run. So
  versions are compared only inside one in-turns record, where every build ran every fixture
  seconds apart ([performance across releases](../user/performance-releases.md)). Never hold a
  `perf/history` figure against another release's, nor one chart against another.
- The same measurement is the trip-wire of a release: a fixture below `0.75` x the last release
  stops it before tagging, on any platform. A trip-wire, not a quality verdict.
- Runners: `ubuntu-latest`, `windows-latest`, `macos-latest`; `machine` names each:
  `GitHub <runner>, <os> <version>, <n> vCPU <cpu model>, rustc <version>`.
- The quality verdict is the tier-2 target table below, measured by hand on a quiet machine.
  CI on pushes and pull requests never benchmarks.

## Commands

```bash
# capture (the Release workflow does this through release.sh bench, on each platform);
# with --out, stdout gets the markdown report of the run (its job summary)
cargo run -p oneterm-vt-bench --release -- all --mib 32 --json --machine "<runner, os, cores, cpu, rustc>" --out perf/history/linux/v0.1.0.json
# this build and others in turns (docs/user/performance-releases.md); --out keeps the figures
cargo run -p oneterm-vt-bench --release -- grid --check --against old/vt-bench --against newer/vt-bench \
    --machine "<runner, os, cores, cpu, rustc>" --out perf/in-turns/linux/v0.1.0.json
# chart of one in-turns record
cargo run -p oneterm-vt-bench --release -- chart --in-turns perf/in-turns/linux/v0.1.0.json --out perf/chart-linux.svg
# where does heavy_sgr spend its time? (hotpath-profiling sites; one fixture per report)
cargo run -p oneterm-vt-bench --release --features hotpath-profiling -- grid --fixture heavy_sgr --mib 32
# per frame (tier 3 reaches snapshot_update)
cargo run -p oneterm-vt-bench --release --features hotpath-profiling -- render --fixture dense_cells
```

## Record schemas

Every field of both records: [record.md](record.md).

## Fixtures (tiers 1-3)

`plain_ascii`, `long_lines` (wrap every line), `heavy_sgr` (24-bit SGR per char), `tui_redraw`
(CUP + short text), `scroll_region` (DECSTBM then line feeds), `cjk_wide`, `dense_cells`
(256-colour SGR per cell), `scrolling` (grid-width lines), `sixel` (small images),
`osc_9_7` (an `OSC 9;7` status message before every line), `grapheme_clusters` (emoji, flags and
combining marks under `?2027`), `hyperlinks` (`OSC 8` links with 200-1000 byte URIs), `mode_churn`
(alt screen and mode toggles, a TUI opening and closing), `sixel_large` (640x480 to 1024x768
images), `unique_styles` (a 24-bit colour of its own for every cell, so the style table grows
with the grid), `unique_links` (a new `OSC 8` link on every line, so the link table is swept),
`kitty_images` (kitty graphics: chunked RGBA images of 48x48 to 128x96, placed three times).
Deterministic generators; no files. Each fixture carries any mode it needs in its own bytes.
A build from before a fixture existed has no figure for it.

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

The seven newest fixtures have no target yet.

## Waivers

A fixture below its pass line, accepted by the owner after a profile. Re-examined each release.

| Fixture | Target | Measured | Ratio | Why (profile summary) | Approved |
|---|---:|---:|---:|---|---|
| - | | | | none | |

Tiers 3-5 have no target: recorded per release, not targeted.
