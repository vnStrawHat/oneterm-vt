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

Informational (phase 12 sets targets). Machine: Intel Core, family 6 model 151 (Alder Lake), 20 logical
CPUs, Windows 11 10.0.26300, rustc 1.96.0, release profile, nothing else building. Command:
`cargo build -p oneterm-vt-bench --release` then `target/release/vt-bench.exe all --mib 32` (the same
run `cargo run -p oneterm-vt-bench --release -- all --mib 32` makes), 160x45, 600 frames of 512 bytes,
median of 5 cycles after 1 warm-up. Tier 3:

| Fixture | us/frame | cells/frame |
|---|---:|---:|
| `plain_ascii` | 4.2 | 1364 |
| `long_lines` | 2.2 | 670 |
| `heavy_sgr` | 1.1 | 186 |
| `tui_redraw` | 16.5 | 5160 |
| `scroll_region` | 20.5 | 6846 |
| `cjk_wide` | 1.6 | 497 |
| `dense_cells` | 1.5 | 204 |
| `scrolling` | 2.2 | 664 |
| `osc_9_7` | 4.9 | 1622 |

Reading it: `cells` is cells copied per frame (a row is 160 cells; 1364 is about 8.5 rows), so cost
follows damage, not the 7,200-cell viewport. A scrolling frame copies the rows that moved in plus the
rows written since the last frame, and a rotated row written this frame is copied again, so a few
lines of output copy more rows than lines. `tui_redraw` and `scroll_region` re-stamp most rows each
frame and pay for it; `dense_cells` is cheap per frame because 512 bytes of 256-colour SGR touch about
one row.
