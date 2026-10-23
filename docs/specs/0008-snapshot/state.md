# `SnapshotState`: `snapshot/state.rs`

One consumer's view of one terminal, reused across frames. Phase 1 (`Terminal::snapshot_update`)
runs under the embedder's lock and copies only rows whose `SeqNo` passed this state's watermark;
phase 2 (`map_colors`, drawing) reads only the state, outside the lock.

## Public types

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Watermark(pub SeqNo);                  // how far this consumer has read the row stamps
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapshotUpdate { Unchanged, Partial { scrolled: i32 }, Full }
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SnapshotCursor { pub id: RowId, pub col: u16, pub row: Option<u16>, pub visible: bool }
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SnapshotPlacement { pub id: GraphicId, pub row: RowId, pub col: u16,
                               pub cols: u16, pub rows: u16, pub pixel_size: (u32, u32) }
#[derive(Debug, Default)] pub struct SnapshotState { .. }   // private fields below
```

- `Partial { scrolled }`: shift your cache by `scrolled` viewport rows (positive = content moved
  up, older rows left the top), then rebuild exactly `changed()`. `Full`: rebuild everything.
- `SnapshotCursor`: `id`/`col` = cursor position; `row` = viewport row, `None` while scrolled
  back past it; `visible` = `ModeSnapshot::show_cursor` (DECTCEM).
- `SnapshotPlacement`: phase 10 fills it (anchor resolved to `row`/`col`); until then never built.

## Fields and engine view

| Field | Type | Meaning |
|---|---|---|
| `generation` | `Option<u32>` | last seen `State.generation`; `None` = never built |
| `palette_epoch`, `mapped_epoch` | `u32`, `Option<u32>` | last seen epoch; epoch `map_colors` last ran at |
| `watermark` | `Watermark` | grid `seq()` at the last non-suppressed update |
| `viewport_top`, `scroll_offset`, `size` | `RowId`, `u32`, `Option<Size>` | as of the last copy |
| `rows`, `changed` | `Vec<SnapshotRow>`, `Vec<u16>` | full viewport; indices this update copied |
| `cursor`, `selection`, `modes`, `placements` | | refreshed every update |
| `hyperlinks` | `FxHashMap<HyperlinkId, Hyperlink>` | strings behind the links in `rows` |
| `force_full`, `meta_dirty`, `rows_copied` | `bool`, `bool`, `u64` | `invalidate`; sticky change hidden by sync; total copies |

```rust
pub(crate) struct EngineView<'a> {
    pub grid: &'a TerminalGrid, pub interner: &'a Interner, pub sync: &'a mut SyncState,
    pub modes: ModeSnapshot, pub generation: u32, pub palette_epoch: u32,
    pub selection: Option<SelectionRange>,     // phase 10 adds `placements: &'a [Placement]`
}
pub(crate) fn begin_update(&mut self, engine: EngineView<'_>, now: Instant) -> SnapshotUpdate;
```

## `begin_update` steps

1. `placements.clear()` (phase 10 refills from live placements with live anchors).
2. Refresh `modes`, `cursor` (active screen: `cursor().pos`, `visible_top()`, `rows()`) and
   `selection`; `meta` = whether any of the three differed.
3. `sync.suppresses_frame(now) && !rows.is_empty()` -> `meta_dirty |= meta`; `changed.clear()`;
   return `Unchanged` (rows, watermark, size untouched). A never-built state is built first.
4. `seq = grid.seq()`; `scrolled = viewport_delta(viewport)`; `full = needs_full(..)`; `changed.clear()`.
5. `full` -> rebuild (resize `rows` to `viewport.rows`, clear `hyperlinks`, copy every row,
   `force_full = false`); else rotate `rows` by `scrolled` (left if positive), then `copy_changed`.
6. Store `generation`, `palette_epoch`, `viewport_top`, `scroll_offset`, `size`, `watermark = seq`.
7. Pick the result (table below); any result other than `Unchanged` clears `meta_dirty`.

`viewport_delta`: `None` when the old and new tops are in different lanes (`>= RowId::ALT_ORIGIN`);
else the signed distance `new.top - old.top` (subtract the smaller `u64` from the larger, then
negate), `None` if it does not fit `i32`. `copy_changed`: per viewport index, copy when
`slot.id != id` (moved in) or `row.seq() > watermark` (written since); push the index;
`rows_copied += 1`.

## Result

| Condition (first match) | Result |
|---|---|
| never built, `invalidate()`, `generation` or `palette_epoch` differs | `Full` |
| `size` differs or `rows.len() != viewport.rows` (resize) | `Full` |
| lane changed (alt swap), or `scrolled.unsigned_abs() >= viewport.rows` | `Full` |
| **D1:** `seq < watermark` (state used with another terminal) | `Full` |
| every row copied (`changed.len() == rows.len()`) | `Full` |
| `changed` empty, `scrolled == 0`, no meta change, `!meta_dirty` | `Unchanged` |
| otherwise (rows, viewport scroll, cursor, mode, selection, held-back meta) | `Partial { scrolled }` |

Engine rule: a row mutated outside `feed` (no `begin_batch`) must be covered by a generation bump.
A state belongs to one `Terminal`; call `invalidate()` before pointing it at another (D1 only
catches a new grid whose `seq` is behind the watermark).

## Counter bumps (`State`, `wrapping_add(1)`)

| Site | `generation` | `palette_epoch` |
|---|---|---|
| `resize`, `set_scrollback_limit`, DECCOLM, DECALN | yes | |
| real alt-screen enter/leave (`?47`, `?1047`, `?1049`) | yes | |
| `?5` when the bit actually changes (no cell changes; the defaults swap) | yes | |
| RIS | yes | if `colors.reset_all()` returned `true` |
| `ColorOverrides::{set, reset, reset_palette}` returned `true` | | yes |
| `set_theme_colors(theme)` with `*theme != State.theme` (**D2**) | | yes |

## Accessors (all `pub`)

| Method | Returns |
|---|---|
| `new() -> SnapshotState` | `Default`; the first update is `Full` |
| `rows() -> &[SnapshotRow]` | the full viewport after the first update, indexed by viewport row |
| `changed() -> &[u16]` | viewport indices this update copied, ascending; empty for `Unchanged` |
| `size() -> Size` | viewport size; `0 x 0` before the first update |
| `viewport_top() -> RowId`, `scroll_offset() -> u32` | first visible row; rows between viewport and bottom |
| `cursor() -> &SnapshotCursor`, `selection() -> Option<SelectionRange>`, `modes() -> ModeSnapshot` | refreshed every update |
| `watermark() -> Watermark` | never decreases while used with one terminal |
| `hyperlink(id: HyperlinkId) -> Option<&Hyperlink>` | strings copied with the rows |
| `placements() -> &[SnapshotPlacement]`, `placement(id: GraphicId) -> Option<&SnapshotPlacement>` | empty / `None` until phase 10 |
| `graphic_offset(id, row: RowId, col: u16) -> Option<(u16, u16)>` | `(col - p.col, row - p.row)` inside `p.cols x p.rows` with `row >= p.row`, else `None` |
| `rows_copied() -> u64` | rows copied since `new`; flat across idle updates |
| `invalidate(&mut self)` | the next non-suppressed update is `Full` |
| `map_colors(&mut self, palette: &Palette)` | `mapped_epoch == Some(palette_epoch)` -> map the `changed` rows; else every row; then `mapped_epoch = Some(palette_epoch)`. Idempotent |

`map_colors` contract (rustdoc): call it after every update, with the palette for the current
epoch; a theme change goes through `Terminal::set_theme_colors`, else mapped rows keep old colours.

## `Terminal::snapshot_update(&mut self, state: &mut SnapshotState, now: Instant) -> SnapshotUpdate`

Builds `EngineView` from `grid`, `interner`, `&mut sync`, `mode_snapshot()`, `generation`, `palette_epoch`,
`selection_range()`; calls `state.begin_update(view, now)`. `&mut self`: the watchdog may close the block.
