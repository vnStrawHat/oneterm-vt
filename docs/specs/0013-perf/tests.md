# Tests and manual checks

Unit tests sit beside the code (`#[cfg(test)] mod tests`). CI runs the bench tests with
`--all-features` too, where `vt-paranoid` and `hotpath-profiling` are on: every test of build
refusals goes through the pure `skew_reason(BuildFlavour)`, never through `cfg!`, so results do
not depend on how the test binary was built. Temp dirs: `std::env::temp_dir()` (the session hook
points it at `.tmp/`), named `vt-bench-check-<pid>-<test>`, removed by the test.

## `regression_check.rs`

| Test | Assert |
|---|---|
| `skew_reason_names_each_flavour` | each of debug, paranoid, profiling alone -> its check.md message; none -> `None`; all three -> the debug message (order 2, 3, 4) |
| `tolerance_range` | `0`, `-0.5`, `1.01`, `x`, `NaN`, `inf` rejected with the check.md text; `0.01`, `0.5`, `1` accepted; absent -> `0.5` |
| `check_takes_only_its_flags` | each of `--json`, `--out F`, `--machine M`, `--frames 5`, `--fixture plain_ascii` -> the "takes only" message |
| `exactly_at_band_passes` | baseline 100.0, now 50.0, tolerance 0.5 -> `ok`; now 49.99 -> `SLOWER` |
| `rows_follow_fixture_order` | baseline entries in reverse order -> rows in `FIXTURES` order |
| `missing_fixture_on_either_side_refuses` | baseline without `sixel` plus an extra `old_name` -> one message naming both; one side empty prints `-` |
| `nonpositive_baseline_refuses` | baseline MiB/s `0.0`, `-1.0`, `NaN` -> refusal 9 naming the fixture |
| `geometry_mismatch_refuses` | record 80x24 -> refusal 7; record with `columns`/`lines` absent (0) -> accepted |
| `newest_record_is_highest_version` | dir with `v0.9.0.json`, `v0.10.0.json` -> `v0.10.0` chosen |
| `empty_or_missing_history_refuses` | empty dir and non-existent dir -> refusal 5 (mentions `--baseline`) |
| `mib_follows_the_baseline` | baseline `mib_per_fixture` 8 -> 8; 0 -> 32; explicit `--mib 4` -> 4 |
| `report_is_exact` | `render_check` on two rows (one `ok`, one `SLOWER`) equals the check.md layout byte for byte; the PASS variant ends with the PASS line |
| `verdict_maps_to_exit_code` | passed report -> 0, failed -> 1 (pure helper); `run(["grid", "--check", "--tolerance", "0"])` == `ExitCode::from(2)` |
| `json_refused_in_skewed_build` | the `--json` guard helper with a skewed `BuildFlavour` -> `refusing --json: <reason>`; clean flavour -> `Ok` |

## `fixtures.rs`, `throughput.rs`, `render.rs`, `main.rs`

| Test | Assert |
|---|---|
| `select_by_name` | `Some("heavy_sgr")` -> exactly that fixture; `None` -> all 10 in table order; `Some("nope")` -> error listing all 10 names in order |
| `fixture_flag_rules` | accepted on `parser`, `grid`, `render`; `all`, `resize`, `rss`, `fixtures` -> unknown-flag error; with `--json` -> the record message |
| `measure_tier_runs_selected_only` | one selected fixture and a fake pass -> 1 row, `RUNS + 1` passes |
| `render_runs_selected_only` | `measure_render(2, &[heavy_sgr])` -> 1 row named `heavy_sgr` |
| `usage_lists_every_command` | still passes; USAGE contains `--tolerance R] [--mib N]` and `--fixture NAME` |

`grid_check_and_render_stay_unimplemented` is deleted: both commands now exist.

## Engine (`crates/vt`)

No new unit tests: the sites are attributes. Coverage comes from builds:

| Command | Proves |
|---|---|
| `cargo clippy -p oneterm-vt --all-targets --features hotpath-profiling -- -D warnings` (CI, Linux) | off half compiles clean |
| `cargo test -p oneterm-vt --features hotpath-profiling` (once, by the coder) | behaviour unchanged with the sites present |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` (CI, 3 OS) | on half compiles clean |

## Manual checks (owner machine; results into `acceptance.md`)

| # | Command (release unless noted) | Expected |
|---|---|---|
| 1 | `vt-bench grid --check` with no `perf/history/` | refusal 5, exit 2 |
| 2 | debug: `cargo run -p oneterm-vt-bench -- grid --check` | refusal 2, exit 2 |
| 3 | `--features vt-paranoid` / `--features hotpath-profiling`, `grid --check` | refusal 3 / 4, exit 2 |
| 4 | `grid --json --machine M --out .tmp/base.json`, then `grid --check --baseline .tmp/base.json` | full table, PASS, exit 0 |
| 5 | copy of base.json with `heavy_sgr` MiB/s x 3, `--baseline` it | `heavy_sgr` `SLOWER`, FAIL line, exit 1 |
| 6 | `--features hotpath-profiling -- grid --fixture heavy_sgr --mib 8` | vt-bench table, then a hotpath table naming `Terminal::feed`, `Parser::advance`, the two scrolls |
| 7 | same with `render --fixture dense_cells --frames 100` | hotpath table also names `Terminal::snapshot_update` |
| 8 | debug `parser --json --machine M` | `refusing --json: ...`, exit 1 |
| 9 | release dry run (history.md) | script reaches `next: git push`; check against the clone's record PASSes |

`echo $?` after each command shows the exit code (Git Bash).
