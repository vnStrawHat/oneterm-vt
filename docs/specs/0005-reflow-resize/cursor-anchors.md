# Cursor, anchors, policy, outcome

## Points: what the column reflow carries

```rust
struct Point { owner: PointOwner, key: Pos, dest: Option<Pos> }   // private, grid/screen/reflow.rs
enum PointOwner { Anchor(AnchorId), Cursor, SavedCursor, ViewportTop }
```

Collected once per column reflow into one `Vec` (capacity = `anchors.live_in(lane) + 3`), sorted by
`key` (row-major), so each logical line claims its points with one moving index:

| Owner | `key` |
|---|---|
| every live content anchor in the primary lane (not screen-owned) | its `pos` |
| `Cursor` | `cursor.pos`, but `col = old_cols` when `pending_wrap` ("past the last column") |
| `SavedCursor` | same rule on `saved_cursor` (Decision 13) |
| `ViewportTop` | `(visible_top, 0)` from the fields (screen-owned entries may be stale; never read) |

`AnchorKind::is_screen_owned` becomes `pub(crate)` for the filter. Duplicate keys are fine (one
point each). Anchors in the alt lane are not collected.

## Mapping through a line

1. `offset = row offset + key.col`, then `min(offset, length)`: a point on trimmed blanks lands at
   the end of its line instead of dying.
2. During the layout walk, the point gets the output position of the cell at `offset`: after a
   `LeadingWideSpacer` insertion (so it follows its glyph to the next row); a point on a
   `WideSpacer` gets the spacer's new column.
3. `offset == length` and the line ended exactly at a row end: `(that row, new_cols)`, i.e. past
   the last column. Otherwise `offset == length` is the cell after the text on the last row.
4. A point whose output row was trimmed by the cap: `dest = None`.

## Applying the results (after the walk, before anything syncs)

| Owner | `Some(pos)` | `None` |
|---|---|---|
| content anchor | `anchors.set(id, (row, min(col, new_cols - 1)))` | new crate `Anchors::kill(id)`: `alive = false`, slot kept (not freed; the owner observes death) |
| `Cursor` | `pos.col == new_cols` -> `col = new_cols - 1`, `pending_wrap = true`; else `pending_wrap = false` | `(newest, 0)`, `pending_wrap = false` |
| `SavedCursor` | same as `Cursor`, on the saved cursor | same |
| `ViewportTop` | remember `pos.row` as the view's top id | remember `oldest` |

Then cursors clamp into the screen (a cursor whose following content took more than a screen
lands on `screen_top`). Screen-owned entries are rewritten by the final `sync_anchors`.

**Alternate screen (clip):** cursor and saved cursor `col = min(col, cols - 1)`; `pending_wrap`
survives only if it was armed and the clamped column is the new last column (shrink); a grow
clears it (the cursor is no longer at the edge).

## Death rules (whole resize)

| Event | Dies |
|---|---|
| reflow output row trimmed by the cap, or a blank row discarded below the cursor | content anchors on it |
| row shrink push + trim, `KeepViewportTop` positive shift trim | content anchors below the new `oldest` (`anchors.trim`) |
| row release (shrink drop, negative shift) | content anchors on released ids |
| alt clip | nobody (columns clamp) |
| anything | screen-owned anchors never die |

## `ResizePolicy`

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum ResizePolicy { #[default] BottomAnchor, KeepViewportTop }
```

| Variant | Meaning (rustdoc) |
|---|---|
| `BottomAnchor` | the bottom of the content stays at the bottom of the window: a grow pulls history in at the top and the cursor moves down with its row. For PTYs that redraw after SIGWINCH (Unix, SSH) |
| `KeepViewportTop` | the screen's top row stays put, as conhost does behind ConPTY: the cursor lands on the row the host re-wraps it to, new rows appear blank at the bottom. For Windows local shells |

**Host row** (`Screen::host_cursor_row(size) -> u16`, crate, primary only, measured **before**
anything moves): cols unchanged -> `min(cursor index, rows - 1)`. Else run the layout walker over
`screen_top..=cursor row` at the new width, where `screen_top` starts a line even if the row
above wrapped into it and the line containing the cursor ends at the cursor row; result = output
row of the cursor key, clamped to `rows - 1`. Reads through `Screen::row`, writes nothing.

**Viewport (both policies):** `offset == 0` before -> `0` after. Else `offset =
screen_top.distance(view_top)` clamped to `history_len` (`view_top` from the table above, or the
unchanged `visible_top` id when no reflow ran). The alternate screen's offset is always `0`.

## `ResizeOutcome`

```rust
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct ResizeOutcome { pub reflowed: bool, pub rows_trimmed: u32 }
```

- `reflowed`: the clamped column count differed, so the primary was re-laid out (true even when
  every line took the fast path).
- `rows_trimmed`: primary rows dropped off the oldest end (reflow cap + row shrink + positive
  shift), saturating. Released bottom rows are not "trimmed". Default = nothing happened.
- No `#[must_use]`: ignoring it is normal.

## `TerminalGrid::resize` order

1. `size = size.clamped()`; equal to both screens' sizes -> `ResizeOutcome::default()`.
2. `begin_batch()`. 3. `target = host_cursor_row` if `KeepViewportTop`. 4. `sticky = offset == 0`.
5. cols changed: primary `reflow_columns` (+trimmed, view top) and alt `clip_columns`.
6. `resize_rows` on primary (+trimmed) and alt. 7. `KeepViewportTop`: primary `shift_cursor_to(target)` (+trimmed).
8. restate primary viewport; `sync_anchors`; `debug_assert` tier-3 `check_integrity(None)`.
