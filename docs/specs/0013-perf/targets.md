# Tier-2 targets: how phase 12 is judged

The targets live in [docs/perf/README.md](../../perf/README.md) ("Tier-2 targets"); this file
does not copy them, it says how to measure against them and what a miss costs.

## Measurement rules

| Rule | Value |
|---|---|
| Machine | the one named in perf/README (i7-12700, Windows 11); nothing else building or running |
| Build | `--release` (the root `[profile.release]`, unchanged: **D4**); no `vt-paranoid`, no `hotpath-profiling` |
| Command | `cargo run -p oneterm-vt-bench --release -- grid --mib 32 --machine "<machine>"` |
| Runs | three back-to-back runs; a fixture's figure is the median of its three MiB/s values |
| Noise | a run whose `spread` exceeds 15% on any fixture is discarded and repeated |
| Pass | figure >= 0.90 x target (perf/README); every one of the ten fixtures, `sixel` included |

## Where we stand (0007 acceptance notes, phase 6 build, one run)

| Fixture | Target | Pass at | Phase 6 | Ratio | |
|---|---:|---:|---:|---:|---|
| `plain_ascii` | 75 | 67.5 | 82.9 | 1.11 | pass |
| `long_lines` | 81 | 72.9 | 85.8 | 1.06 | pass |
| `heavy_sgr` | 227 | 204.3 | 178.5 | 0.79 | **miss** |
| `tui_redraw` | 126 | 113.4 | 126.0 | 1.00 | pass |
| `scroll_region` | 61 | 54.9 | 61.8 | 1.01 | pass |
| `cjk_wide` | 115 | 103.5 | 143.1 | 1.24 | pass |
| `dense_cells` | 196 | 176.4 | 157.6 | 0.80 | **miss** |
| `scrolling` | 80 | 72.0 | 86.3 | 1.08 | pass |
| `sixel` | 47 | 42.3 | - | - | first measured in phase 10 |
| `osc_9_7` | 102 | 91.8 | 118.4 | 1.16 | pass |

Phases 7-11 add damage tracking and graphic draining to `feed`, so phase 12 re-measures first
(step 1 below) and works from that table, not this one. Expect the two SGR-heavy fixtures to
be the work: their streams are one SGR per character or cell, so SGR dispatch and style
interning are the first suspects (a hypothesis for step 2, not a finding).

## Procedure for a miss

```text
1. measure    three runs per the rules above -> table in "Acceptance notes"
2. profile    hotpath recipe (hotpath.md) on the missing fixture alone: --fixture NAME
3. fix        smallest change that moves the figure; re-run step 1 for ALL fixtures
4. or waive   owner-approved waiver row in docs/perf/README.md
```

- A fix lands as its own commit `perf(<scope>): <what>, <fixture> <before> -> <after> MiB/s`
  (docs/agents/commits.md), body naming this spec. It must not lower any other fixture below
  its pass line, and must keep the quality gate and every test green.
- A fix that needs a public API change, a new dependency, or `unsafe` stops and goes back to
  the architect (docs/agents/workflow.md); it is not done inside phase 12.
- A waiver needs a profile first (step 2). The coder or orchestrator drafts the row; only the
  owner approves it. Agents never approve a waiver.

## Waiver record (`docs/perf/README.md`, new section after "Tier-2 targets")

```markdown
## Waivers

A fixture below its pass line, accepted by the owner after a profile. Re-examined each release.

| Fixture | Target | Measured | Ratio | Why (profile summary) | Approved |
|---|---:|---:|---:|---|---|
```

`Measured` = the step-1 figure; `Why` = one sentence naming the dominant site and why fixing it
is out of proportion now; `Approved` = `owner, YYYY-MM-DD`. When no waiver exists the table
holds one row `| - | | | | none | |`, so the section is never silently missing.

## Tiers 3-5

No targets (perf/README). Phase 12 records one `all --mib 32` run in the acceptance notes;
from v0.1.0 on, each release record is their baseline. perf/README's sentence "phase 12 records
the first numbers as their baseline" becomes "recorded per release, not targeted; compare
releases on one machine".

## Acceptance notes

The coder creates `acceptance.md` in this folder (<= 120 lines) with: machine, rustc, date; the step-1
table (target, three runs, median, ratio, verdict) before and after each fix; one `all --mib 32`
run (tiers 1-5); the hotpath report summary per profiled fixture (top 5 rows); the list of
`perf(...)` commits; any waiver row as approved.
