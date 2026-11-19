# `grid --check --machine`: the machine of the run

Added with the Release workflow's `--machine`; the same-machine band was added after v0.4.1.

## Why

The first design assumed one baseline machine. The Release workflow runs the check on hosted
runners, and that assumption failed twice:

1. One runner label is several CPU models: a run on a slower model read every fixture at 0.55 to
   0.76 of the baseline and one at 0.48, with no change to that fixture's cost relative to the
   others on that model (the v0.4.0 release run).
2. One description is not one speed either. The macOS runner, same description to the letter,
   measured `plain_ascii` at 529, 927 and 286 MiB/s in three release runs; the last had no change
   to the engine since the one before, and the Linux job of that run read 0.91 to 1.28. Held as
   they were, 4 of 17 fixtures fell below the band and stopped the v0.4.2 release.

## Rules (only when `--machine` is given)

- **Same machine** = equal text after dropping a trailing `, rustc <version>` (`hardware()`).
- **Default baseline**: the newest record of the same machine that passes refusals 6a-9; if
  there is none, the newest record.
- **The band** lies around a centre `c`; a fixture is `SLOWER` when `ratio < tolerance * c`,
  exactly at it passes, and a fixture without a baseline is never slower. With `m` the median of
  `now / baseline` over the judged fixtures:

| Baseline (default or `--baseline`) | `Band` | `c` | Reason |
|---|---|---|---|
| no `--machine` | `Baseline` | 1 | by hand, on the machine that recorded the baseline |
| the same machine | `SameMachine` | `min(1, m)` | a slower run of the runner moves every ratio; a faster run must not hide a fixture that kept its old figure, so the band never rises above the baseline |
| another machine | `OtherMachine(text)` | `m` | figures of two machines cannot be held against each other |

Blind spot, printed in the docs: a slowdown shared by most fixtures moves the median and is not
told from a slower machine or a slower run. The hand-measured target table is the verdict for
that (targets.md).

## Output

Another machine: one more line after `Baseline machine:`, another rule and verdict.

```text
Baseline machine: <baseline's>
This machine: <TEXT>
Run: 160x45, 32 MiB per fixture, median of 5 cycles after 1 warm-up.
The baseline is another machine's: fixtures are judged against the median ratio of this run, 0.59.
A fixture fails below 0.50 x that median (0.30); exactly at it passes.
...
PASS: 17 of 17 fixtures at or above 0.50 x the median ratio
FAIL: 1 of 17 fixtures below 0.50 x the median ratio: `heavy_sgr`
```

The same machine: no `This machine:` line. The verdict names `the median ratio` when `c < 1`,
`baseline` when `c = 1`.

```text
Run: 160x45, 32 MiB per fixture, median of 5 cycles after 1 warm-up.
The baseline is this machine's, and one runner description is not one speed: the band follows the median ratio of this run, 0.55, when it is below 1.
A fixture fails below 0.50 x 0.55 (0.28); exactly at it passes.
...
PASS: 17 of 17 fixtures at or above 0.50 x the median ratio
```

## Tests (`regression_check.rs`)

`hardware_is_the_machine_without_its_toolchain`; `the_baseline_is_the_newest_record_of_the_same_machine`
(also the `Band` chosen); `a_record_of_the_same_machine_that_is_no_baseline_is_passed_over`;
`a_named_baseline_of_another_machine_is_judged_against_the_median`;
`another_machine_is_judged_against_the_median_of_its_run`;
`a_slower_run_of_the_same_machine_lowers_the_band` (the v0.4.2 run's shape passes; half of what
the others hold is the edge; a faster run leaves the band at the baseline);
`the_median_ratio_skips_fixtures_without_a_baseline`; `report_for_another_machine_is_exact`;
`report_for_a_run_of_the_same_machine_is_exact`.
