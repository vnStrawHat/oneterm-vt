# `grid/screen.rs`: ring, viewport, cursor, region, scrolling

Module doc: the **screen** is the bottom `rows` rows and every VT op addresses it by index;
the **viewport** is what the user sees, `offset` rows above the screen. No error, no panic:
counts clamp, out-of-range reads are blanks.

## Types

```rust
pub enum ScreenKind { Primary, Alternate }                   // origin(): PRIMARY_ORIGIN / ALT_ORIGIN
pub enum Charset { #[default] Ascii, SpecialCharacterAndLineDrawing }   // ESC ( B / ESC ( 0
pub enum CursorOrigin { #[default] Screen, Region }          // DECOM reset / set
pub struct Cursor { pub pos: Pos, pub pending_wrap: bool, pub charsets: [Charset; 4],
                    template: Cell, erase: Cell }            // pub fn erase(&self) -> Cell
pub struct ScrollRegion { pub top: u16, pub bottom: u16 }    // bottom exclusive; in grid/mod.rs
pub struct RowsScrolled { pub top: RowId, pub bottom: RowId, pub delta: i32 } // inclusive ids
pub struct ScrollReport { pub scrolled: Option<RowsScrolled>, pub trimmed: Option<RowId>,
                          pub history_rows: u16 }
pub struct TabStops { stops: Vec<bool> }   // pub fn new(cols), pub fn clear(&mut self, col)
```

All derive `Clone, Copy, PartialEq, Eq, Hash, Debug` (`TabStops`: `Clone, PartialEq, Eq, Debug`).
`ScrollRegion`: `full(rows)`, `height()` (saturating), `contains(index)`.
`Cursor.template` = pen a printed glyph inherits; `erase` = `Cell::EMPTY` + the pen's background
only (`set_template(template, &mut Interner)` interns `Style { bg, ..Style::DEFAULT }`). The erase
paths never touch the interner. `TabStops`: stop every 8 columns; crate `is_stop`, `set`, `clear_all`.

## `Screen` fields

`kind, slots: Box<[Option<Row>]>, mask, oldest, newest, rows, cols, scrollback_limit, offset: u32,
region, cursor, saved_cursor, tabs, seq, batch_lo: RowId, spare: Option<Row>, dropped_wide: u32,
cursor_anchor, saved_cursor_anchor, viewport_anchor`. Manual `Debug` (summary, never the ring).

- `slots.len() = (scrollback_limit + MAX_ROWS).next_power_of_two()`, fixed for the screen's life
  (resize in phase 4 never re-masks). `scrollback_limit` clamps to `SCROLLBACK_MAX`.
  Slot = 48 B: `DEFAULT_SCROLLBACK` (10,000) -> 16,384 slots = 768 KiB; alt (0) -> 1,024 slots.
- Live ids are `oldest..=newest`; `newest - oldest + 1 >= rows`. A fresh screen: `oldest =
  origin`, `newest = origin + rows - 1`, all slots `None`, cursor and saved cursor at screen top.
- `spare`: the last row a trim dropped, kept for its allocation; never read as content.

## Public methods

| Method | Meaning |
|---|---|
| `new(kind, size, scrollback_limit, &mut Anchors) -> Screen` | size via `Size::clamped`; registers its 3 anchors |
| `kind, rows, cols, size` | geometry |
| `oldest, newest, row_range() -> Range<RowId>` | live ids (`oldest..newest + 1`) |
| `history_len() -> u32` | live rows above the screen |
| `screen_top()` / `visible_top()` | `newest - (rows - 1)` / `screen_top - offset` |
| `scroll_offset()` | `offset`; 0 = following output |
| `scroll_viewport(delta: i32)` | negative = towards history; clamps to `0..=history_len` |
| `scroll_to_bottom()` | `offset = 0` |
| `row(id) -> RowRef` | slot holds a row with that id -> it, else blanks |
| `row_mut(id) -> Option<RowMut>` | `None` outside `oldest..=newest` (a non-live id shares a slot with a live row); else materialise with `Cell::EMPTY` (not the erase cell) via `recycle(spare)` |
| `row_of_index(index) -> RowId` | clamps index to the screen |
| `cursor()`, `saved_cursor()`, `cursor_row_index()`, `cursor_anchor()` | cursor state |
| `goto_origin(index, col, CursorOrigin)` | CUP; `Region`: index + region.top, clamped to region.bottom - 1 |
| `set_region(top, bottom, CursorOrigin) -> bool` | DECSTBM, 0-based, bottom exclusive, clamps bottom; `top >= bottom` -> `false`, no change; else homes via `goto_origin(0, 0, origin)` |
| `wrapline(&mut Anchors)`, `dropped_wide()`, `row_text(..)` | see screen-edit.md |
| `reset(&mut Anchors)` | RIS for this screen: `ED 3`, cursor + saved cursor home with `Cell::EMPTY` pens, full region, default tabs, offset 0, reset all screen rows. Ids are not reset |
| `ring_len()`, `ring_mask()` | `slots.len()`, `slots.len() - 1` |
| `heap_bytes() -> usize` | `slots.len() * size_of::<Option<Row>>()` + every row's `bytes()` + spare + tabs capacity |

Crate: `goto(index, col)` (clears pending wrap), `carriage_return`, `backspace` (col 0: no-op that
keeps `pending_wrap`; else col - 1 and clear it), `save_cursor` / `restore_cursor` (whole `Cursor`;
restore clamps), `set_template`, `region()`, `tabs_mut()`, `index_of(id) -> Option<u16>`,
`viewport() -> Viewport`, `set_seq(seq)` (batch open: `seq`, `batch_lo = screen_top`),
`sync_anchors(&self, &mut Anchors)`.

## Scroll primitives (crate; each ends with `sync_anchors` and the bounded check)

Region arguments clamp to the screen; `n = n.min(region.height())`; `n == 0` -> empty report,
nothing stamped. Anchor effects: anchors.md.

| Op | Rule |
|---|---|
| `scroll_up(region, n, anchors)`, top == 0 | **push** `n` ids (below). Cursor and saved cursor ids `+= n` (same screen index). `offset != 0` -> `offset += n`, clamped (a scrolled-back view holds still). Full region: no row moves, `scrolled = None`. `bottom < rows`: the tail rows below the region keep their screen index: **rotate** them onto ids `+ n`, reset the `n` vacated region-bottom ids; `scrolled = {top: old tail top + n, bottom: newest, delta: +n}`. `history_rows = n`; `trimmed = Some(oldest)` if a trim happened |
| `scroll_up`, top > 0 | **rotate** ids `top..bottom` up by `n`, reset the last `n`; `scrolled = {region ids, delta: -n}`; nothing enters history |
| `scroll_down(region, n, anchors)` | mirror: rotate down, reset the first `n`; `delta = +n`; never pulls from history |
| `linefeed(anchors)` | `index + 1 == region.bottom` -> `scroll_up(region, 1)`; else `< rows` -> down one; else stay (cursor below the region). Pending wrap untouched |
| `reverse_index(anchors)` | `index == region.top` -> `scroll_down(region, 1)`; else `> 0` -> up one |
| `insert_lines(n)` / `delete_lines(n)` | cursor outside the region -> `None`, nothing changes; else `scroll_down` / `scroll_up` on `{index, region.bottom}` (DL at index 0 feeds history, as above) |
| `clear_history(anchors)` (ED 3) | `offset = 0`; slots `oldest..screen_top` -> `None` (keep one as `spare`); `oldest = screen_top`; trim anchors; report `trimmed` |

**Push** one id: `newest += 1`; while `history_len > scrollback_limit`: take the `oldest` slot's
row into `spare`, `oldest += 1`. The new slot: `None` if the cursor's erase cell is `Cell::EMPTY`
(an unseen id needs no stamp), else `recycle(spare)` filled with the erase cell.

**Rotate** (no allocation): first materialise every `None` slot in the moved range (`recycle(spare)`
or a fresh row; at most `rows` rows, once), then move rows with `slots.swap(slot(i), slot(i + n))`
(ascending for up, descending for down), re-home each moved row (`id`, `seq`, `DIRTY`), and
`Row::reset` the vacated rows in place with the erase cell under their new id. Every slot touched
ends holding a row whose id matches (no stale rows, paranoid.md).

Allocation claim (tested): a full-screen scroll-up with full history allocates nothing (no row is
touched except the trimmed one, which becomes `spare` and backs the next materialised row); a
region rotation over already-allocated rows allocates nothing.
