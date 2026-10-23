# Files to touch

## `crates/vt`

| Action | Path | Content |
|---|---|---|
| edit | `Cargo.toml` | `[dependencies] memchr.workspace = true`; `[dev-dependencies] proptest.workspace = true` |
| edit | `src/lib.rs` | replace "this release has no public items yet" with one line naming `parser`; add `pub mod parser;` and the `#[cfg(doctest)]` chapter include (tests.md) |
| create | `src/parser/mod.rs` | module doc (what the parser is, untrusted input, "truncate, never error"); `mod` decls; `pub use` of the API list; `DCS_MAX_BYTES`; `Dispatch`; `Parser` with `new`/`advance`/`reset` and the shared private helpers (clear, collect, digit, separator, push pending, csi/dcs/osc dispatch) |
| create | `src/parser/state.rs` | private `enum State`; one `fn` per non-ground state (tables in state-machine.md); anywhere rules; DCS/APC byte cap |
| create | `src/parser/ground.rs` | `memchr` run scan, UTF-8 decode loop, C1 rule, print-run splitter, UTF-8 carry |
| create | `src/parser/params.rs` | `Params`, `ParamSep`, `ParamGroups`, `Intermediates`, `MAX_PARAMS`, `MAX_INTERMEDIATES` |
| create | `src/parser/osc.rs` | private `OscBuffer`; `OscParams`, `StringTerm`, `MAX_OSC_PARAMS`, `OSC_INLINE`, `OSC_LARGE`; capacity unit tests |
| create | `src/parser/parser_tests.rs` | unit tests, `mod props` (tests.md) |
| create | `tests/parser_limits.rs` | integration tests (tests.md) |

Rules for this code:

- Every `pub` item has rustdoc (crate is `#![warn(missing_docs)]`); rustdoc names no repo paths.
- No `unwrap`/`expect`/`panic!`/indexing that can panic on input. The "ground is never passed to
  the per-byte table" invariant is a `debug_assert!` plus a no-op in release, not a panic.
- `advance` is generic (`D: Dispatch + ?Sized`); no `dyn` and no allocation on the hot path.
- `#[inline]` only where tier 1 shows it helps; record any such choice in a "why" comment.
- No `hotpath` attributes yet (phase 12).

## `crates/vt-bench`

| Action | Path | Content |
|---|---|---|
| create | `src/fixtures.rs` | `COLUMNS`, `LINES`, `Fixture`, `FIXTURES`, the 10 generators, `write_fixtures(dir, mib)` |
| create | `src/throughput.rs` | `RUNS`, `CHUNK`, `NullDispatch`, `measure_tier`, `parser_pass`, `summarize`, `render_table` |
| edit | `src/perf_history.rs` | `Serialize` + new fields (below); `pub(crate) fn utc_date(unix_seconds: u64) -> String` |
| edit | `src/main.rs` | `parser`, `all`, `fixtures` commands and flags (bench-fixtures.md); printing stays in `main.rs` |

No new dependencies (`oneterm-vt`, `anyhow`, `serde`, `serde_json` are already there).

`perf_history.rs` structs after this phase (field order = JSON key order = schema order):

```rust
#[derive(Serialize, Deserialize)]
pub(crate) struct PerfRecord {
    #[serde(default)] engine: String, version: String, #[serde(default)] date: String,
    machine: String, #[serde(default)] columns: u16, #[serde(default)] lines: u16,
    #[serde(default)] mib_per_fixture: u32, #[serde(default)] runs: u32, tiers: Tiers,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Tiers {
    #[serde(default, skip_serializing_if = "Vec::is_empty")] tier1_parser: Vec<Throughput>,
    #[serde(skip_serializing_if = "Vec::is_empty")] tier2_grid: Vec<Throughput>, // still required on read
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Throughput {
    fixture: String, mib_per_second: f64, #[serde(default)] ns_per_byte: f64, spread_percent: f64,
}
```

`#[serde(default)]` keeps phase-0 test records and the chart working. `utc_date` uses the
days-to-civil algorithm (no new crate).

## `vt-bench` unit tests

| Module | Test |
|---|---|
| fixtures | `make(1)` golden bytes for all 10 (prefix + unit 0 from the table; `dense_cells` checks its first 32 bytes and length) |
| fixtures | `heavy_sgr`: unit 159 ends with `\r\n`, unit 160 does not; `tui_redraw` unit 1 is `ESC[2;8Hstat: 42` |
| fixtures | determinism (two calls equal); `target <= len < target + longest unit` for `target = 100_000`; names unique, table order |
| fixtures | each fixture, `make(1 << 16)` through `Parser` + a counting sink: no `ignore`, no truncated OSC, no aborted DCS; `heavy_sgr` CSIs == printed chars; `osc_9_7` OSCs == LFs; `sixel` hooks == unhooks == LFs; `dense_cells` CSIs == printed chars + frames |
| throughput | `summarize` on 1 MiB with 5, 1, 3, 2, 4 ms -> 333.3 MiB/s, 2.861 ns/B, spread 240% (within 0.1); a zero duration does not divide by zero |
| throughput | `render_table` contains the header row and one row per fixture |
| perf_history | `utc_date`: `0` -> `1970-01-01`, `951782400` -> `2000-02-29`, `1792713600` -> `2026-10-23`, `4107542400` -> `2100-03-01` |
| perf_history | a tier-1-only record serializes with `tier1_parser`, without `tier2_grid`, keys in schema order |
| main | `--json` without `--machine` is an error naming `--machine` (if argument parsing is a pure fn) |

## Docs

| Action | Path |
|---|---|
| create | `docs/user/parser.md` (coder; outline in tests.md) |
| done by architect | `docs/user/README.md` row 14, `docs/README.md` spec table row |
| edit | this spec's bench-fixtures.md "Acceptance notes" (tier-1 numbers) |

