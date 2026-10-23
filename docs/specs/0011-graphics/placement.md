# Placements and graphic lifetime: `graphics.rs`, `graphics/placement.rs`

Three rules (module doc): a cell stores *which* image (one interned `Extras { graphic }` per
image), never where inside it; a placement is an anchor (`AnchorKind::Graphic`), so it moves with
its content through every row-moving primitive and reflow; liveness is derived from rows, not
from a counter (a counter misses row resets, blanking scrolls, alt wipes and reflow).

## Public items

```rust
pub const VIRTUAL_CELL: (u16, u16) = (10, 20);    // fallback cell (VT340, conhost), see below
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GraphicData { pub id: GraphicId, pub width: u32, pub height: u32, pub rgba: Vec<u8> }
#[non_exhaustive] #[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement { pub id: GraphicId, pub anchor: AnchorId, pub cols: u16, pub rows: u16,
                       pub pixel_size: (u32, u32) }   // read only: no API takes one
impl Terminal {
    pub fn take_graphics(&mut self) -> Vec<Arc<GraphicData>>;   // mem::take, oldest first
    pub fn placements(&self) -> &[Placement];                   // live placements, oldest first
    pub fn set_cell_pixels(&mut self, width: u16, height: u16); // stores State.cell_pixels
}
// VtEvent::GraphicReleased(GraphicId)   (VtEvent is non_exhaustive: minor)
```

`GraphicData`: RGBA8, row-major, stride `width * 4`, straight alpha, `width`/`height <= 4096`.
`Placement`: `anchor` names the top-left cell; `cols` already clipped on the right; `rows` is
the full height (not clipped at the bottom); `pixel_size` = decoded size.

Crate: `GraphicsState { next_id: u64 (starts at 1), pending: Vec<Arc<GraphicData>>,
pending_bytes: usize, placements: Vec<Placement>, released: Vec<GraphicId>, decoder:
Option<SixelDecoder> }` in `State.graphics`. Constants (crate): `MAX_DIMENSION = 4096`,
`MAX_PLACEMENTS = 256`, `PENDING_MAX_BYTES = 128 MiB` (**D2**).

## Cell size

Effective cell = `cell_pixels` when both axes are `> 0`, else `VIRTUAL_CELL` (a half-set `(9, 0)`
falls back whole; nothing divides by zero). `set_cell_pixels` changes no existing footprint and
bumps no counter. `CSI 14 t` answers `rows * cell_h ; cols * cell_w` of the **effective** cell
(**D4**; edits 0007 queries.md). Power-on `cell_pixels = (0, 0)`; RIS keeps it (embedder state).

## `place(handler, image: DecodedSixel)` (from `dcs_unhook`, active screen)

1. `(cw, ch)` = effective cell; `walk = min(band_pixels / ch, u16::MAX)`; `id = next_id`
   (`saturating_add`; never reused; RIS keeps the counter).
2. `cursor` = active cursor; `cols = min(ceil(width / cw), screen_cols - cursor.col)`;
   `rows = max(ceil(height / ch), 1)`; both `u16` via `try_from`, saturating.
3. `only = State::intern_extras(Extras { hyperlink: None, graphic: Some(id) })` (reclamation.md).
   `ExtrasId::NONE` (table full) -> still placed, no cell names it; counted by the existing
   extras delta in `hyperlink_table_exhausted`.
4. While `placements.len() >= MAX_PLACEMENTS`: release `placements[0]` (below).
5. `anchor = anchors.register(Graphic(id), cursor)`; push the `Placement`.
6. For `r in 0..max(rows, walk + 1)`: `line` = the cursor row now when `r <= walk`, else that
   row `+ (r - walk)`; if `r < rows` and `line` is on the active screen, **stamp** it; if `r <
   walk`, run the handler's LF path (scroll report -> events and `rows_scrolled`;
   `lines_produced += 1`). Rows below the final cursor row are stamped without scrolling and
   clipped at the bottom.
7. **D3:** release every other placement whose extent lies inside this one's (positions read
   from the anchors after step 6; `row..row + rows` and `col..col + cols` both contained).
8. Push `Arc<GraphicData>` to `pending` (cap below), after the cells: no pixels for an image
   that is not on the grid.

**Stamp** (one row): for each column of `col..col + cols` that exists: cell extras `NONE` ->
`only`; else `merged = *resolve_extras(e)` with `graphic = Some(id)` -> `interner.extras(merged)`
(no sweep: the row borrow is held, and the id is written before anything can sweep).
`RowMut::repair(col, cell.with_extras(e))` keeps text, style and the wrap flag; then
`mark_graphic()`. Opening the row stamps its `SeqNo` (damage). Text printed later over a cell
replaces the cell and its id.

**Cursor rule** (DEC and conhost; not xterm's "column 0 below the image"): it walks `band_pixels
/ cell_h` rows and keeps its column, so it lands on the image's last row, or below it when the
payload ends in `-` and the cell height divides the image. 384x576 with 96 bands: at 9x18 ->
43x32 cells, cursor row 31; at 10x20 -> 39x29, row 28; with a trailing `-` at 9x18 -> row 32. A
raster size that disagrees with the data moves the footprint, not the walk.

## Release

`release(i)`: remove the placement, `anchors.release(anchor)`, push its id to `released`.

| Cause | Where |
|---|---|
| **sweep**: anchor dead, or no row of `anchor.row .. anchor.row + rows` on `screen_of(anchor.row)` has `HAS_GRAPHIC` | end of every `feed` (after the grapheme sweep) and `Terminal::resize` |
| eviction of the oldest at `MAX_PLACEMENTS` | `place` step 4 |
| covered by a newer image (**D3**) | `place` step 7 |
| its pixels dropped from the queue (**D2**) | `place` step 8 |

`released` is drained into the batch at the end of every `feed`, an empty one included (which
otherwise pushes nothing): one `GraphicReleased(id)` per release, in release order, before
`Repaint`. A release made by `resize` is delivered by the next `feed`.

| Grid event | Outcome |
|---|---|
| LF / IND / SU into primary history | anchor follows; live in history |
| trim out of history (limit, ED 3, `set_scrollback_limit`), alt scroll-out | anchor killed -> next sweep releases |
| ED 2 on the primary | graphic cells are not erasable (0004): rows go to history, live |
| ED 2 on the alt screen, RIS, alt wipe (`?1047 l`, `?1049 h`), a region scroll by its height | rows reset -> released |
| IL / DL / SU / SD / region scroll inside the image | anchor moves with the top row; the extent may split; stray rows keep naming the id |
| text or EL / ECH over its cells | those cells drop the id; the row flag stays, so it stays live (renderers paint by cells, snapshot.md) |
| alt swap | the hidden screen's placements stay live in their lane |
| resize / reflow | anchor remapped or killed (0005); `resize` sweeps |

**RIS:** `pending` cleared, `pending_bytes = 0`, decoder dropped; placements are released by the
end-of-feed sweep (rows reset), so each fires exactly once. `extras.clear()` stays (0007).

## Hand-off and caps

- `take_graphics` is the only drain: several `SnapshotState`s may exist, and a frame skipped by
  `?2026` loses nothing. It resets `pending_bytes`. Embedder order per cycle: `feed` ->
  `take_graphics` (upload) -> handle the batch (`GraphicReleased` frees) -> `snapshot_update`.
  Pixels of an image released before it was taken are still handed out once (that order frees
  them).
- **D2:** before the push, while `pending_bytes + rgba.len() > PENDING_MAX_BYTES` and `pending`
  is not empty: drop the oldest pending image, release its placement if still live,
  `graphics_dropped += 1` (new `FeedStats` field, events.md row: "images whose pixels were
  dropped before `take_graphics`"). Its `GraphicReleased` names an id the embedder never received.
- Bounds: decode buffer <= 64 MiB plus one transient `rgba` <= 64 MiB in `finish`; queue <= 128
  MiB (a single image is <= 64 MiB, so the newest always fits); 256 live placements; one extras
  entry per image still named by a cell (reclamation.md).
