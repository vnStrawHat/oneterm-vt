# Bench tier 3 `render`: `crates/vt-bench/src/render.rs`

What a renderer pays per frame: tier 2's `feed` between frames (untimed) plus one timed
`snapshot_update` per simulated frame, with one `SnapshotState` reused across frames (a fresh
state per frame would measure only `Full` rebuilds). Informational until phase 12.

## Measurement

```rust
pub(crate) const DEFAULT_FRAMES: usize = 600;
pub(crate) const FRAME_BYTES: usize = 512;            // output arriving between two paints
pub(crate) struct FrameCost { fixture: String, us_per_frame: f64, cells: u64 } // perf_history
pub(crate) fn measure_render(frames: usize) -> Vec<FrameCost>;
```

Per fixture in table order, `sixel` skipped until phase 10 (same filter as tier 2), `RUNS + 1`
cycles interleaved across fixtures like `measure_tier` (cycle 0 discarded):

1. Untimed: `Terminal::new(Size { rows: LINES, cols: COLUMNS }, Config::default())`, one
   `EventBatch`, one `SnapshotState`, `now = Instant::now()` (one value for the whole cycle).
2. Prime (untimed): feed the `dense_cells` fixture made at `COLUMNS * LINES * 20` bytes in
   `CHUNK`s, then one `snapshot_update`, so every viewport cell holds content.
3. `stream = (fixture.make)(frames * FRAME_BYTES + 1024)`; `copied_before = state.rows_copied()`.
4. For each frame: `term.feed(&stream[f * FRAME_BYTES..][..FRAME_BYTES], &mut batch, now)`
   (untimed); time only `black_box(term.snapshot_update(&mut state, now))`.
5. Cycle result: `spent / frames` in microseconds; `cells = (rows_copied() - copied_before) *
   COLUMNS / frames` (**D3**: cells actually copied per frame, computed outside the timer).

`us_per_frame` = median of the `RUNS` cycle results; `cells` = the value of the median cycle (it
is deterministic, so every cycle agrees). `--frames N` sets `frames`.

## CLI and record (`main.rs`, `perf_history.rs`)

| Invocation | Runs |
|---|---|
| `render [--frames N] [--json --machine T] [--out F]` | tier 3 (`--mib` accepted, unused) |
| `all [...]` | tiers 1, 2, 3, 4, 5 in that order, one report / one record |

- `--frames` now takes a value: a whole number `1..=100_000`, default `DEFAULT_FRAMES`; anything
  else -> `--frames needs a whole number 1..=100000, got '<v>'` (exit 1). `has_frames` goes away.
- `Tiers` gains `tier3_render: Vec<FrameCost>` between `tier2_grid` and `tier4_resize`,
  `#[serde(default, skip_serializing_if = "Vec::is_empty")]`. Field names = the perf/README schema
  (`fixture`, `us_per_frame`, `cells`); no new field.
- Text section:

```text
## Tier 3: parse + grid + snapshot_update per frame (600 frames)

| Fixture | us/frame | cells/frame |
|---|---:|---:|
| `plain_ascii` | 3.1 | 160 |
```

  `us/frame` `{:.1}`, `cells/frame` integer.

## Expected shape (asserted only loosely, see tests.md)

| Fixture | Typical damage per frame |
|---|---|
| `plain_ascii`, `long_lines`, `scrolling` | a few rows scrolled in: `Partial { scrolled }`, a few rows copied |
| `tui_redraw`, `heavy_sgr`, `dense_cells` | scattered rows; `dense_cells` often every row (`Full`) |
| `scroll_region` | rows inside the region re-stamped, viewport still |

## Unit tests (`vt-bench`)

| Test | Assert |
|---|---|
| `render_skips_sixel` | `measure_render(4)` -> 9 rows in table order, no `sixel` |
| `render_values_are_sane` | every `us_per_frame` finite and `>= 0`; every `cells <= COLUMNS * LINES` |
| `idle_frames_copy_nothing` | the frame loop with an empty slice per frame -> `cells == 0` (private helper taking the stream) |
| `frames_flag_parses` | `--frames 0`, `--frames x` rejected; `--frames 5` accepted |
| `record_has_tier3` | a record with tier 3 serialises `tier3_render` between `tier2_grid` and `tier4_resize`; one without omits it |

## Acceptance notes

Machine, command (`cargo run -p oneterm-vt-bench --release -- all --mib 32`) and the tier-3 table
are pasted here by the coder after the phase lands (informational; phase 12 sets targets).
