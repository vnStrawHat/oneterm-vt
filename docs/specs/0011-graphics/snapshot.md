# Snapshot side: `snapshot/state.rs`, `snapshot/row.rs`

Phase 7 shipped the final shapes (`SnapshotPlacement`, `SnapshotCell::graphic`, `placement(s)`,
`graphic_offset`); phase 10 fills them. No new public item.

## Filling

```rust
pub(crate) struct EngineView<'a> { /* 0008 fields */ pub placements: &'a [Placement] }
// SnapshotState gains a private `scratch: Vec<SnapshotPlacement>` (capacity reused)
```

`Terminal::snapshot_update` passes `&self.state.graphics.placements`. `begin_update` step 1
(0008 state.md) becomes:

1. `scratch.clear()`; for each `Placement` in engine order (oldest first = draw order) whose
   `anchors.get(anchor)` is `Some(pos)`: push `SnapshotPlacement { id, row: pos.row, col:
   pos.col, cols, rows, pixel_size }`. A dead anchor is skipped (the next sweep releases it).
2. **D5:** `placements_changed = scratch != placements`; `swap(scratch, placements)`. Step 2's
   `meta` becomes "modes, cursor, selection **or placements** differed".

Placements refresh on every update, a suppressed one included (0008 Decision 2); a change hidden
by `?2026` sets `meta_dirty` like any other meta change. Steady state allocates nothing (two
vectors at their high-water mark, <= 256 entries each).

| Field | Source |
|---|---|
| `SnapshotCell::graphic` | unchanged rule (0008 rows.md): the cell's extras `graphic`. May name a released image (a stray row after IL, a cell outliving an eviction): `placement(id)` is then `None` and the cell draws no image |
| `SnapshotCell::hyperlink` | unchanged; a merged entry carries both |
| `placements()` | the refreshed list; both lanes (an alt-lane placement while the primary is shown has rows outside the viewport) |
| `placement(id)` | linear find (<= 256), `None` when released |
| `graphic_offset(id, row, col)` | unchanged (0008): `(col - p.col, row - p.row)` inside `p.cols x p.rows` with `row >= p.row`, else `None` |

Rustdoc drops every "None until phase 10" note (rows.md field comment, state.md accessors).

## Damage when an image appears or dies

| Case | Rows copied | Result (0008 table) |
|---|---|---|
| image placed in the viewport | stamped rows (their `SeqNo` moved) | `Partial` (or `Full` if every row changed); placements changed -> meta |
| image whose walk scrolled the screen | rows moved in + stamped rows | `Partial { scrolled }`, or `Full` when `scrolled >= rows` |
| image placed while scrolled back, out of view | none in view | `Partial` (placements meta), `changed()` may be empty |
| released by a row reset (ED 2 alt, RIS, wipe, blanking scroll) | the reset rows | `Partial`, or `Full` via the generation bump (RIS, alt swap) |
| released with no viewport row change (eviction, cover, trim, queue drop) | none | `Partial { scrolled: 0 }`, empty `changed()` (**D5**; alternative B returns `Unchanged`) |
| `set_cell_pixels` | none | no change (nothing on the grid moved) |
| suppressed frame (`?2026`) | none | `Unchanged`, meta held in `meta_dirty` for the next real frame |

## What a renderer does (rustdoc example on `SnapshotState::placements`, user chapter)

- Upload `take_graphics()` results keyed by `id`; free on `GraphicReleased(id)`.
- Draw by cells: for a cell with `graphic: Some(id)` and `graphic_offset(id, row.id, col) ==
  Some((dx, dy))`, draw the texture positioned at `cell_origin - (dx, dy) * cell_size` (one
  image pixel per device pixel, `SnapshotPlacement::pixel_size`), clipped to that cell. Cells
  whose text replaced the image, and split rows, then draw correctly with no extra rule.
- Or draw by placement (simpler, ignores overwrites): the texture at the placement's top-left
  cell, clipped to `cols x rows`, when `row` is in the viewport range.
- Text is drawn on top of image cells; the engine never rescales pixels.

## Edits to spec 0008's tests

`graphics_hook_is_empty` is replaced by the snapshot rows of tests.md (`placement_refills`,
`stale_graphic_has_no_placement`, `placement_only_change_is_partial`, ...).
