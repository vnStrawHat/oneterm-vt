# Why builds in turns: the evidence

Behind [check.md](check.md): what was tried, what the runners did, and what the band and the
three rounds rest on.

## Records do not compare (first design, through v0.4.2)

The check held a run against the newest `perf/history` record, and the chart drew one record
per release. On hosted runners a record says what the runner did that minute:

| Release run | What happened |
|---|---|
| v0.4.0 | the label landed on a slower CPU model: every fixture at 0.55 to 0.76 of the record |
| v0.4.2, first run | the same runner description to the letter, nothing changed in the engine: every fixture at 0.31 to 0.63 |
| three releases, one macOS label, unchanged engine | `plain_ascii` at 529, 927 and 286 MiB/s |

A band tied to the median ratio of the run let v0.4.2 through and is blind to a slowdown of
every fixture. So figures of two versions compare only when both are measured side by side.

## A whole table at a time is too far apart (second design, never released)

The last release and this build each ran the whole tier-2 table per round, about 40 s apart.
Builds of the same engine sources, best of five rounds:

| Job | this / last release |
|---|---|
| Linux | 0.92 to 1.07 |
| Windows | 0.96 to 1.01 |
| macOS, a quiet job | 0.96 to 1.10 |
| macOS, a job at half speed throughout (`plain_ascii` 489 MiB/s, not 939) | 0.96 to 1.23, 12 of 17 fixtures 5% or more off |

What the machine did within those 40 s fell on one build and not the other.

## A fixture at a time (this design)

Rehearsal of the release bench job, one job per OS: the five releases v0.2.1 to v0.4.2 and a
build of the same engine sources as v0.4.2, 32 MiB, five rounds.

| | Linux | Windows | macOS |
|---|---|---|---|
| same sources / v0.4.2, best of 5 rounds | 0.96 to 1.08 | 0.99 to 1.01 | 0.87 to 1.10 |
| the same, best of the first 3 rounds | 0.94 to 1.09 | 0.98 to 1.02 | 0.81 to 1.10 |
| the same, first round alone | 0.87 to 1.09 | 0.92 to 1.32 | 0.77 to 1.56 |
| one build, slowest round / best round, worst fixture | 0.45 | 0.68 | 0.46 |
| five rounds of six builds | 12 min | 24 min | 17 min |
| the first three of them | 7 min | 15 min | 10 min |

- Rounds of one build differed by a factor of two inside a job, and the best of five rounds
  of two builds still agreed to 1%, 8% and 13%: the noise fell on every build alike.
- Three rounds are the default, chosen by the owner for the time of a release job: they read
  the same builds 2%, 9% and 19% apart.
- `0.75` stays the band, with little room on macOS: the worst reading of an unchanged engine
  in three rounds was 0.81, from one job. A release stopped at a ratio near the band is run
  again, or with `--rounds 5`. A chart line that moves by less than its platform's margin
  says nothing.
- Some of the 8% on Linux is not noise: `kitty_images` read 1.08 to 1.09 in every round count,
  on unchanged engine sources, so it is the layout of the code around them. The same pair read
  0.92 on another CPU model.
- What a real change looks like beside that: `kitty_images` from v0.2.1 to v0.4.2 reads 1.79 x
  on Linux, 2.49 x on Windows and 2.44 x on macOS in the same records.

## The timed functions are out of line

`throughput::grid_pass` and `parser_pass` are `#[inline(never)]`. With a second caller of
`grid_pass` in the bench crate the same engine sources read `plain_ascii` at 0.80 and 0.76 on
macOS in two jobs (0.99 on Linux and Windows), and 1.01 out of line. What is timed must not
depend on the bench's own call graph.
