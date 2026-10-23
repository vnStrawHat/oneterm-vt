# Files to touch

Builds on specs 0002-0007 as implemented; phase 6 (`Terminal`) lands first. No new dependency
(`rustc-hash` is already a `vt` dependency). Sub-milestones, gate green after each: **7a** types,
rows, palette, mode snapshot, `SnapshotState`, `Terminal` wiring and counter bumps; **7b** sync;
**7c** bench tier 3 and user chapters.

## `crates/vt/src` (new)

| Path | Content | Milestone |
|---|---|---|
| `snapshot.rs` | module doc (pull model, two phases, one state per consumer); `mod` decls; re-exports; `pub(crate) use state::EngineView` | 7a |
| `snapshot/state.rs` | `Watermark`, `SnapshotUpdate`, `SnapshotCursor`, `SnapshotPlacement`, `SnapshotState`, `EngineView` (state.md) | 7a |
| `snapshot/row.rs` | `StyleRun`, `SnapshotContent`, `SnapshotCell`, `SnapshotRow`, `copy_from` (rows.md) | 7a |
| `snapshot/palette.rs` | `Palette`, default table (palette-modes.md) | 7a |
| `snapshot/mode_snapshot.rs` | `ModeSnapshot`, `MouseProtocol`, `MouseReporting`, `MouseEncoding` | 7a |
| `snapshot/sync.rs` | `SyncState`, `SYNC_REFRESH`, `SYNC_WATCHDOG` (sync.md) | 7b |
| `snapshot/snapshot_tests.rs`, `snapshot/sync_tests.rs` | tests.md; `#[cfg(test)] mod snapshot_tests;` in `snapshot.rs`, `#[cfg(test)] mod sync_tests;` in `sync.rs` (the fixture lives in `snapshot_tests.rs`, `pub(super)`) | 7a, 7b |

## `crates/vt/src` (edits)

| Path | Change |
|---|---|
| `terminal.rs` | `State` gains `sync: SyncState`, `theme: Palette` (`Palette::new()`), `generation: u32`, `palette_epoch: u32`; methods `snapshot_update`, `sync`, `mode_snapshot`, `mouse_reporting`, `set_theme_colors`, `palette` (D4); `feed` passes `now` to the handler; `resize` and `set_scrollback_limit` bump `generation` |
| `terminal/dispatch.rs` | `Handler { state, out, now: Instant }`; DECALN (`ESC # 8`) bumps `generation` |
| `terminal/modes.rs` | `?2026` bit removed from `Modes`; `?2026 h/l` -> `sync.begin(now)` / `sync.end()`; `Terminal::mode` and DECRQM read `sync.is_set()`; crate `Modes::mouse_protocol()`; `generation` bumps at DECCOLM, real alt enter/leave, a real `?5` change; RIS calls `sync.end()` and bumps per state.md |
| `terminal/color.rs` | every arm whose `set` / `reset` / `reset_palette` returns `true` bumps `palette_epoch` |
| `terminal/query.rs` | DECRQM `?2026` row: state from `sync.is_set()` (bytes unchanged: `1` / `2`) |
| `grid/terminal_grid.rs` | `pub(crate) fn seq(&self) -> SeqNo` (the current batch stamp); nothing becomes `pub` |
| `lib.rs` | below |

`lib.rs` (the crate doc gains one sentence on damage-tracked snapshots and `?2026`):

```rust
mod snapshot;
pub use snapshot::{ModeSnapshot, MouseEncoding, MouseProtocol, MouseReporting, Palette, SnapshotCell,
    SnapshotContent, SnapshotCursor, SnapshotPlacement, SnapshotRow, SnapshotState, SnapshotUpdate,
    StyleRun, SyncState, Watermark};
// + #[cfg(doctest)] includes of docs/user/snapshot.md, threading.md, quickstart.md (same pattern)
```

`SnapshotPlacement` is exported now (`placements()` returns it); phase 10 fills it and only adds
`Placement`, `GraphicData`, `VIRTUAL_CELL` and the `EngineView.placements` field.

## `crates/vt-bench/src`

| Path | Change |
|---|---|
| `render.rs` (new) | `DEFAULT_FRAMES`, `FRAME_BYTES`, `measure_render`, tests (bench.md) |
| `main.rs` | `render` command; `all` runs tier 3 between 2 and 4; `--frames N` parsed (`has_frames` removed); `render` no longer "not implemented yet" |
| `perf_history.rs` | `FrameCost`; `Tiers.tier3_render` in schema order |

## Docs

| Path | Change |
|---|---|
| `docs/user/snapshot.md`, `threading.md`, `quickstart.md` | new chapters (tests.md outlines); flip rows 01, 02, 18 of `docs/user/README.md` to `ready` with links |
| `docs/perf/README.md` | field table: `cells` = tier 3 cells copied per frame (D3); no schema change |
| this spec's `bench.md` | acceptance notes |

## Rules for this code

- Every `pub` item has rustdoc (`#![warn(missing_docs)]`); rustdoc names no repo paths or spec
  files. `SnapshotUpdate`, `rows()`, `changed()` and `map_colors` docs state the renderer contract.
- No `unwrap`/`expect`/`panic!`/`unreachable!`/`Instant + Duration` outside tests; index with
  `get` (`style_of`, `cluster`), `u16`/`u32` conversions with `try_from` or a documented bound.
- Never hold a borrow of `Terminal` in `SnapshotState`: it owns values only (`Send + Sync`).
- No `#[allow]`; the `cfg_attr(.., expect(dead_code))` rules of 0007 files.md apply.

## Commit

`feat(snapshot): damage-tracked snapshots, palette, mode snapshot and synchronized output` (minor).
