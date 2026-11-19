# Performance across releases

A figure in MiB/s holds for one machine in one minute: one hosted runner measured an unchanged
engine a factor of three apart from run to run. So releases are compared only where they were
measured side by side, and that measurement is also the gate before a release. The verdict on
speed stays the table in [performance](performance.md), measured by hand on a quiet machine.

## Builds measured in turns

```bash
git worktree add .tmp/previous v0.4.2
(cd .tmp/previous && cargo build -p oneterm-vt-bench --release --target-dir target)
cargo run -p oneterm-vt-bench --release -- grid --check --against .tmp/previous/target/release/vt-bench
```

Runs tier 2 of this build and of every build given. In a round, every build runs a fixture
before any runs the next one, each run a process of its own: the builds of one fixture are
measured seconds apart, so what the machine does over minutes falls on all of them alike. A
figure is the best of the rounds, because a machine can only slow a round down.

- `--against BINARY`, once per build, oldest first: a `vt-bench` built from a release. This
  build is held against the last one given; the others are measured for the table and the chart.
- `--tolerance R` (default `0.75`): a fixture fails below `R` x that last build, exactly `R`
  passes. `--rounds N` (default 3), `--mib N` (default 32).
- `--machine TEXT --out FILE`: also writes the figures of every build, with every round, as an
  in-turns record ([docs/perf/record.md](../perf/record.md)).
- A build runs only the fixtures it generates byte for byte as this build does: a figure of
  another stream would say nothing about the engine. Its cell shows `-`; a fixture the last
  build given did not run is `not judged`.
- Exit codes: `0` pass, `1` at least one fixture `SLOWER`, `2` no verdict (bad flag; debug,
  `vt-paranoid` or `hotpath-profiling` build; a binary that does not run; no fixture in common
  with the last build given).
- The build refusals read `vt-bench`'s own features; `--features oneterm-vt/vt-paranoid` on the
  command line bypasses them, so do not.

It catches a build that became slower in time: a clone per cell, a lost fast path, a hot loop
that fell out of the cache. It is a trip-wire, not a fine measure: on hosted runners two builds
of the same engine sources read within 2% of each other on Windows, 9% on Linux and 19% on
macOS. Read a chart line with that margin in mind.

## What a release does with it

The Release workflow, on each platform, before tagging:

| Step | What |
|---|---|
| build | the last five releases, with the job's compiler, beside the new build |
| measure | all six in turns, as above; a fixture below `0.75` x the last release stops the release (workflow input `throughput_tolerance`) |
| record | `perf/in-turns/<platform>/v<version>.json`: every build, every fixture, every round |
| chart | `perf/chart-<platform>.svg`, drawn from that record alone: one line per fixture across the six builds |

- Every point of a chart comes from one job on one machine, so a line that moves by more than
  the margin above is the code.
  The chart of one release cannot be laid over the chart of another: they are two jobs.
- A release older than the last five leaves the chart; its figures stay in the in-turns records
  of the releases that measured it.
- Five, not all: every earlier release costs the job a build and its share of every round. Six
  builds take 7 minutes on the Linux runner, 10 on macOS and 15 on Windows.
- The same job stores the release's own tier 1-5 run in `perf/history`. Its MiB/s say what that
  runner did that minute; they are not held against another release.
- The job summary shows the in-turns table, then the full run.
