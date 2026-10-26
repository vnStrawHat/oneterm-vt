# 13g: one perf history per platform (key, layout, record, `vt-bench`)

Owner request 2026-10-26: the release bench runs on Linux, Windows and macOS like the tests;
history records and charts are split per platform. No release exists yet, so there is nothing to
migrate and no compatibility path.

## Platform key and layout

| Item | Decision |
|---|---|
| key | `linux`, `windows`, `macos`: Rust `std::env::consts::OS` on the three runners (its std docs give these values); in `release.sh`, `RUNNER_OS` (`Linux`, `Windows`, `macOS` per the GitHub docs on default variables) lower-cased |
| records | `perf/history/<platform>/v<version>.json` |
| charts | `perf/chart-<platform>.svg` |
| release assets | `chart-<platform>.svg` (file name as is), `v<version>-<platform>.json` (renamed when staged) |

Why folders for records: `load_history(dir)` already reads every `*.json` of one folder, so one
folder per platform needs no new loading code. Why flat `chart-<platform>.svg`: `gh release
create` names an asset after the file's base name (GitHub CLI manual), so three `chart.svg`
would collide; with distinct file names the charts upload unrenamed and sit side by side in `perf/`.

## Record field `platform` (schema change)

New required string after `date`, before `machine`: `"platform": "linux"`. Written by every
`--json` run from `consts::OS`; no `#[serde(default)]`, so a record without it is invalid.

Why the field as well as the folder: a file in the wrong folder, or a release asset downloaded
alone, still says where it was measured, and `vt-bench` can refuse to compare across platforms:

| Where | Refusal |
|---|---|
| `grid --check`, any baseline (default or `--baseline`) | `{src}: baseline platform {p}, this build runs on {host}` (exit 2) |
| `chart --history DIR` | `chart needs records from one platform, found: linux, windows` (sorted, deduplicated) |

## `vt-bench` changes (no new command, no new flag)

| File | Item | Change |
|---|---|---|
| `perf_history.rs` | module doc | `Reading perf/history/<platform>/*.json: one record per release.` |
| `perf_history.rs` | new `pub(crate) const PLATFORM: &str = std::env::consts::OS;` | doc: the platform this binary runs on, as records name it |
| `perf_history.rs` | `PerfRecord` | field `platform: String` between `date` and `machine`, required |
| `main.rs` | `render_json` | `platform: perf_history::PLATFORM.to_owned()` |
| `main.rs` | `USAGE` | `compare tier 2 with the newest perf/history/<os> record`; chart line `SVG of tier-2 MiB/s per release of one platform, one line per fixture` |
| `main.rs` | `chart_command` | `chart::render_svg(&releases)?` |
| `chart.rs` | `render_svg(&[Release]) -> Result<String>` | refusal above; title `oneterm-vt tier 2 throughput on {platform} (MiB/s, higher is better)`; machine warning unchanged |
| `regression_check.rs` | `HISTORY_DIR` doc | "Parent of the per-platform record folders" |
| `regression_check.rs` | `check_command` | default folder `Path::new(HISTORY_DIR).join(PLATFORM)` |
| `regression_check.rs` | `newest_record` | refusal `no perf record in perf/history/{PLATFORM}; the Release workflow writes the first, or pass --baseline FILE`; source `perf/history/{PLATFORM}/v{version}.json` |
| `regression_check.rs` | `check_baseline` | platform refusal first, before geometry |

`prepare_check(args, flavour, history)` keeps its signature: tests pass a temp folder, and the
host platform is the constant. `consts::OS` is fixed at compile time for the target, so the
"host" is the build target.

## Tests (unit, in the same modules)

| Module | Test | Change |
|---|---|---|
| `perf_history` | `record` helper JSON | adds `"platform":"linux"` |
| `perf_history` | `tier1_only_record_serializes_in_schema_order` | literal gets `platform`; key list `... "date", "platform", "machine" ...` |
| `perf_history` | `record_has_tier3` | literal gets `platform` |
| `perf_history` | new `record_without_platform_is_invalid` | `parse_release` of a record lacking `platform` errs and names the source |
| `chart` | `release` helper JSON | adds `"platform":"linux"` |
| `chart` | the four `render_svg` tests | `.unwrap()` the result |
| `chart` | new `title_names_the_platform` | SVG contains `throughput on linux` |
| `chart` | new `mixed_platforms_are_refused` | one record's `record.platform` set to `windows`: error text exact |
| `regression_check` | `release` helper | `platform: PLATFORM.to_owned()` |
| `regression_check` | `newest_record_is_highest_version` | source `perf/history/{PLATFORM}/v0.10.0.json` |
| `regression_check` | `empty_or_missing_history_refuses` | new refusal text (with `{PLATFORM}`) |
| `regression_check` | `report_is_exact` | source and expected line `perf/history/linux/v0.1.0.json` (cosmetic, matches check.md) |
| `regression_check` | new `foreign_platform_baseline_refuses` | baseline with platform `elsewhere`: exact refusal text, via `check_baseline` and via `prepare_check --baseline` |
| `main` | `json_record_names_the_platform` (new) | build a `PerfRecord` via `render_json` with no tiers measured (`Measured::default()`): JSON contains `"platform": "<PLATFORM>"` |

`docs/specs/0013-perf/check.md` is amended in place (default folder, refusal 5 text, new
platform row); its other rules stand.
