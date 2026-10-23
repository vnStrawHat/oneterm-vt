# Algorithm, memory, integrity

## Why a scratch list, not in-place emission

Output ids start above every old id, and each output row lands in slot `id & mask`. A column shrink
produces more rows than it reads, so writing straight into the ring overtakes unread source rows
once the free slots (as few as `ring_len - scrollback - rows`, possibly 0) run out. Detaching the
live rows first makes the ring empty and every write conflict-free, for grow and shrink alike, at
the cost of one pointer move per live row.

## `TerminalGrid` scratch

New private field `reflow_rows: Vec<Option<Row>>` (`Default` empty). The only memory a resize keeps:
empty between calls, capacity kept (bounded by `scrollback_limit + MAX_ROWS` entries of 48 B), and
added to `TerminalGrid::heap_bytes` (`capacity * size_of::<Option<Row>>()`). Passed to
`Screen::reflow_columns(cols, anchors, &mut self.reflow_rows)`.

## Passes of `Screen::reflow_columns`

1. **Points**: collect and sort (cursor-anchors.md). One `Vec` per call, O(P log P).
2. **Detach**: for `id in oldest..=newest`, `scratch.push(slot.take())` (index = `id - oldest`).
   The ring is now all `None`. Remember `old_cols`, `rows`, `cap`, `base = newest_issued + 1`.
3. **Walk** the scratch forward, one logical line at a time (columns.md), lazily: a line is found
   by scanning `WRAPPED` flags; nothing is materialised per line. For each line:
   - compute `length` (trim + cursor extension), claim its points (moving index);
   - fast path -> move the `Row`; else run the layout walker and copy cell spans into recycled
     buffers;
   - store each output row with `store(row: Option<Row>)`: id = `base + k`, write the slot; if the
     live count exceeds `cap`, take the oldest output slot (its row becomes `spare`), `oldest += 1`,
     `trimmed += 1`.
4. **Cursor, pad**: discard trailing `None` rows below the cursor while its row is above the
   screen (columns.md); then pad with `None` rows until `newest - oldest + 1 >= rows`.
5. **Install**: `cols = new_cols`, `tabs.resize`, `newest_issued = newest`; drop the scratch
   leftovers except one kept as `spare` (if `spare` is empty); `scratch.clear()`.
6. **Apply points** (cursor-anchors.md), clamp cursors, tier-1 `debug_assert`.

The layout walker is one private function shared with `host_cursor_row`; it reads cells through
`RowRef` (scratch entries via `RowRef::allocated` / `RowRef::blank`, the ring via `Screen::row`)
and reports each output row as a span, so it never allocates:

```rust
struct RowSpan { start: usize, end: usize, leading_spacer: bool, wrapped: bool } // line offsets
fn lay_out(line: &LineCells<'_>, length: usize, cols: u16, on_row: impl FnMut(RowSpan));
```

`LineCells` (private) = the line's rows and their contributions; a monotonic reader turns a span
into per-source-row slices for `extend_from_slice` (one slice per source row the span crosses).
The emitter maps points to spans; the measure only counts spans.

## Row buffers (no per-row `Vec`)

An output row with content takes a buffer in this order, then `clear()`, `reserve_exact(new_cols)`,
`extend_from_slice` per span slice, pad with `Cell::EMPTY`, `Row::from_cells(..)`:

1. `spare` (holds a row trimmed by the cap a moment ago, so a full-history shrink runs at steady
   state with no allocation);
2. the pool: scratch entries of lines already finished (`split_at_mut` at the current line start;
   a cursor scans forward for `Some`, amortised O(1));
3. a new `Vec` (only when output rows outnumber recycled ones, i.e. a shrink).

New crate items in `row.rs`: `fn into_cells(self) -> Vec<Cell>` and
`fn from_cells(id, seq, cells: Vec<Cell>, flags: RowFlags, occ: u16) -> Row` (`debug_assert`
length; the integrity walk checks pairs, `occ`, hints afterwards). `Row::fit` (columns.md).

## Complexity

| Part | Cost |
|---|---|
| detach, install | O(live rows) pointer moves |
| fast-path line | O(1) + trailing-blank scan inside `occ` (+ one realloc per row on grow) |
| other lines | O(cells copied); nothing proportional to `cols` beyond the copied prefix and padding |
| points | O(P log P) sort + O(P) claims |
| rows step, shift | O(rows changed) + O(rows) blank-tail checks |
| host measure | O(cells of the screen rows above the cursor) |

Ceiling: output rows that the cap later trims are still built. A backward counting pass could
skip them; add it only if the tier-4 shrink case at full history shows it.

## Allocation claim (tested, tests.md)

- No allocation besides: the point `Vec`, scratch growth (first resize, or a deeper history), row
  buffers beyond the recycled ones, and `Row::fit` reallocations on a grow.
- A column change where every line takes the fast path and `new_cols <= old_cols` keeps every row
  buffer (same addresses).

## Integrity after a resize

- Each screen step ends with the tier-1 `debug_assert`; `TerminalGrid::resize` ends with tier 3
  (`check_integrity(None)`), like `reset` and `enter_alt` in spec 0004.
- `begin_batch` ran first, so `batch_lo` is an old id below the new `oldest`; the bounded walk
  clamps to `oldest` and covers the whole live range after a reflow. Rows-only resizes walk from
  `min(batch_lo, visible_top) - 1` as usual. `vt-paranoid` adds the stale-slot scan, which proves
  the detach left no row behind.
- Tests run `check_integrity(Some(&interner))` after every resize (fixture `check()`).
