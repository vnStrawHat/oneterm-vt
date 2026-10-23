# Row resize (`grid/screen/resize.rs`)

`Screen::resize_rows(rows, anchors) -> u32` (crate; returns rows trimmed off the oldest end).
Runs on both screens after the column step; same rules for both (the alternate screen simply has
`scrollback_limit == 0`, so whatever it pushes is trimmed at once and not reported).

## Grow by `k`

1. `pull = min(history_len, k)`: `rows += pull` with `newest` unchanged moves `screen_top` up by
   `pull`; those rows leave history and keep their ids.
2. `rows += k - pull`, and as many blank rows are appended at the bottom through the push
   helper with `Cell::EMPTY` (not the pen's erase cell: nothing was erased).
3. Cursor and saved cursor keep their row ids, so their screen index grows by `pull`.

## Shrink by `k`

Let `c` = cursor screen index, `blank_tail` = consecutive blank rows counted up from the bottom,
strictly below the cursor row. Blank = unallocated, or `occ == 0` and last cell `Cell::EMPTY`
(O(1) per row).

```text
push = min(c, k - min(k, blank_tail))   // rows that go to history from the top
drop = k - push                         // rows released from the bottom, all below the cursor
```

- `drop <= blank_tail` unless `push` was capped by `c`: content below the cursor is pushed into
  history, not lost, as long as the cursor row can stay on screen (Decision 6).
- Cursor low (on the bottom row): `blank_tail = 0`, `push = k`, the classic scroll into history.
- Cursor high over blank rows: they are dropped, nothing enters history.
- **Push**: lowering `rows` with `newest` fixed moves `screen_top` down; the rows above it are
  history now (ids kept); then trim to `scrollback_limit` (count, `anchors.trim`).
- **Drop**: ids `newest - drop + 1 ..= newest` are released: slots set to `None` (one buffer may
  become `spare`), content anchors on them die (`shift_region(kill, 0, kill)`), `newest -= drop`.
  The new bottom row loses `WRAPPED` (a line never continues past `newest`).
- Saved cursor: if its row went to history or was released, clamp to the screen (row
  `screen_top` or `newest`, column kept).

## Released ids (open item 1)

New field `Screen::newest_issued: RowId` = the highest id this screen has ever had live
(initialised to `newest`; never decreases).

- The push helper stores `None` for a new id only when `id > newest_issued` and the erase cell is
  `Cell::EMPTY`; an id `<= newest_issued` (released earlier) is materialised and stamped like a
  written row, so `RowRef::seq` of a re-issued id is never older than its release.
- Every path that raises `newest` raises `newest_issued` to match. Reflow output starts at
  `newest_issued + 1`, so it never reuses a released id.
- Tier 1 adds `newest <= newest_issued` (`IdsOutOfOrder`).
- `grid/mod.rs` module doc and `docs/user/grid.md`: "ids never rewind, except that a resize may
  release rows below the cursor; a released id comes back only as a freshly stamped row".

## Always, on every resize of a screen

| Item | Rule |
|---|---|
| scroll region | reset to `ScrollRegion::full(rows)` (also when only `cols` changed) |
| tab stops (cols changed) | new crate `TabStops::resize(cols)`: truncate; extend with the default rule of `TabStops::new` at absolute columns, so stops cleared in the kept prefix stay cleared |
| `offset` | restated at the end of `TerminalGrid::resize` (cursor-anchors.md), never left past `history_len` |
| cursors | clamped into `screen_top..=newest`, `col < cols` (tier 1 holds after every step) |

## Bounds: `Size::clamped`

`resize` clamps first: rows `1..=MAX_ROWS` (1024), cols `1..=MAX_COLS` (2048). Sizes come from
remote hosts, so `0` or `u16::MAX` are normal input, never an error. The clamped size is compared
with both screens' sizes for the early return. The ring is never re-sized: `rows <= MAX_ROWS`
keeps `history + rows <= ring_len`.

## `KeepViewportTop` shift (primary only, after both screens resized)

`Screen::shift_cursor_to(target, anchors) -> u32` (crate, returns trimmed). `e` = cursor index:

| `e - target` | Action |
|---|---|
| `> 0` (`n`) | append `n` blank rows at the bottom (push helper): the screen scrolls up `n`, top rows enter history (trim, counted) |
| `< 0` (`n`) | `n = min(n, history_len, rows below the cursor)`: release `n` bottom rows (as Drop above, content may be lost: the host cut them too), pull `n` rows from history |
| `0` | nothing |
