# Column reflow (`grid/screen/reflow.rs`)

Primary screen only, only when `cols` changes. Module doc: a logical line is what the program
printed between two line ends; rows are one layout of it, and a width change re-lays it out.

## Logical lines

- A line = a maximal run of live rows `a..=b` where every row before `b` has `WRAPPED`; `b` ends
  it (its flag is clear, or `b == newest`). Unwritten rows are `Cell::EMPTY` x `cols`, flags empty.
- **Row contribution** = its cells, minus a trailing `LeadingWideSpacer` (a placeholder for a glyph
  that wrapped; the layout re-inserts one wherever the new width needs it).
- **Line offset** of `(row, col)` = sum of earlier rows' contributions + `col` (`usize`: a line can
  span the whole history).
- **Length** = offset after the last cell that is not `Cell::EMPTY` (trailing trim), raised to the
  cursor's offset on the cursor's line (Decision 4: the cursor keeps its distance from the text).
  The trim scans back from the last row: a row whose last cell is not `EMPTY` ends the scan at
  its full contribution; else scan back from `occ - 1` (cells at `col >= occ` equal the last
  cell); an all-`EMPTY` row moves the scan one row up. Cost = trailing blanks inside `occ`, not `cols`.

## Layout at `new_cols` (one walker, used by reflow and by the host measure)

Walk offsets `0..length`, filling output rows left to right:

| Case | Rule |
|---|---|
| `Wide` at `i` (its `WideSpacer` is `i + 1`), `new_cols >= 2`, one column left | put a `LeadingWideSpacer` (= `Cell::EMPTY.with_width(LeadingWideSpacer)`) in the last column, end the row `WRAPPED`, place the pair on the next row |
| `Wide` / `WideSpacer`, `new_cols == 1` | degrade each half to a blank `Narrow` (style kept); count one `dropped_wide` per glyph |
| any other cell | copy verbatim (style, extras, grapheme ids, semantic, protected bits unchanged) |
| row full | end it; `WRAPPED` iff `i < length` |
| end of line | end the open row unwrapped (pad `Cell::EMPTY`); `length == 0` emits one blank row |

- A pair is never split: the integrity walk guarantees `Wide` + `WideSpacer` sit in one source row,
  and the trim never cuts a spacer (it is not `EMPTY`).
- Graphemes and extras survive because cells are copied by id; no interner access, no re-intern.
- Output row header: `DIRTY`, `WRAPPED` per the table, hints = union of `CONTENT_HINTS` of the
  source rows that contributed cells (a false positive is allowed, never a false negative;
  `HAS_GRAPHIC` carried this way), `occ` = cells written (copied + spacer), `seq` = batch stamp.
- A row of the output with nothing written (blank line) is stored as `None` (no buffer).

## Fast path (no copy)

A line that is one source row, holds a row, `1 <= length <= new_cols`, and whose last kept cell is
not a `LeadingWideSpacer` is **moved**: `Row::fit(new_cols)` (truncated cells are all `EMPTY`;
padding is `EMPTY`), `Row::rehome(new_id, seq)`. Points on it map by the general rule (an offset
past `length` clamps to `length`). Most real scrollback takes this path.

## `RowId` promise

| Resize | Ids |
|---|---|
| same clamped size | nothing happens (early return, no batch, no stamp) |
| cols changed, primary | every live row gets a new id: `base = newest_issued + 1`, output `k` = `base + k` (moved fast-path rows included); all old ids die; `oldest`/`newest` = first/last surviving output id |
| rows only | ids kept (rows.md) |
| alternate screen | ids kept; cells clipped or padded in place |

Why fresh ids: a reflow changes the row count above most rows, so an old id would name different
content. Ids stay contiguous; consumers follow content with anchors (cursor-anchors.md).

## Scrollback cap during reflow

`cap = scrollback_limit + rows` (current rows; the row resize runs after). The cap is applied
**after** the cursor rule and padding below, so padding never evicts content: then the oldest
output rows over `cap` are dropped (buffers recycled, algorithm.md); each drop adds 1 to
`rows_trimmed`; points that landed on it die. Storing drops early only if the ring is full.

**Cursor row stays on screen** (as in the row shrink, Decision 6): while the cursor's output row
is above the last `rows` output rows (or the output is over `cap`) and the newest output row is
`None` and below the cursor, discard it (points on it die; its id never became live). If none is
left, the cursor clamps.
Without this, padding rows added by a grow would push the cursor off screen on the way back.
Then `None` rows are appended until the screen has `rows` rows (content stays at the top when
there is no history to pull). `cap <= ring_len` always holds (ring sized `>= scrollback + MAX_ROWS`).

## Alternate screen: clip / pad, no reflow

`Screen::clip_columns(cols, anchors)`: every allocated live row gets `Row::fit(cols)`; `WRAPPED`
kept; ids kept; tab stops resized; anchors in the alt lane get `col = min(col, cols - 1)`; cursor
rules in cursor-anchors.md. Rationale: a full-screen app redraws on SIGWINCH; reflowing its
grid would only produce garbage it overwrites.

## `Row::fit(cols)` (crate, `row.rs`)

Truncate or pad with `Cell::EMPTY`; then repair the edge: a `Wide` left in the last column becomes
a blank `Narrow`; a `LeadingWideSpacer` no longer in the last column becomes a blank `Narrow`;
`occ` = `min(occ, cols)` if the old last cell was `EMPTY`, else `min(old_cols, cols)`;
`DIRTY` set, `WRAPPED` kept. Capacity is kept on truncation (Decision 12).
