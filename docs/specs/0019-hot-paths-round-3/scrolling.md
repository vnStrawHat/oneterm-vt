# S5 line feed into history, S6 boxed row slots (grid)

## What rio does per line feed that we do not (`short_lines`, `y CR LF`)

Pinned, the compare adapters run with `grid --scrollback N` (one pass = 32 MiB, ms):

| Engine | scrollback 10,000 | scrollback 100 (rows stay in cache) |
|---|---:|---:|
| rio | 249 | 183 |
| oneterm-vt (HEAD) | 550 | 455 |

- Cold recycled rows cost both engines about the same (rio 66 ms, us 95 ms): both reset the
  oldest row's prefix at the moment it is reused, a cache miss on a row last touched 10,000
  lines ago. Not the gap.
- The gap is CPU work on hot rows: 16 ns per line for rio, 40 ns for us. rio's line feed is
  `rotate` (an index) plus one row reset; ours runs the general region scroll for every line:
  `clamp_scroll`, the `n`-loop, the tail-rotation and offset branches, a walk over every
  anchor entry in `Anchors::trim` (6 screen-owned entries even with nothing else
  registered), `sync_anchors`, a 48-byte `ScrollReport`. Parser alone: 9 ns per line.
- Events: one `RowsTrimmed` per line once history is full (0018: removing all scroll events
  +4 %; not proposed, events are API).

## S5 design (`grid/screen.rs`, `grid/anchor.rs`)

| Item | Change |
|---|---|
| `Screen::linefeed` | at the region bottom, when the region is the whole screen (`top == 0`, `bottom == rows`) and `offset == 0`: `linefeed_into_history`; otherwise `scroll_up` as today |
| `Screen::linefeed_into_history` (new, private) | `scroll_up_into_history` for `n = 1` without a tail rotation: `push_row(erase)`, both cursors' rows `+ 1`, `trimmed` = new `oldest` when it moved (then `anchors.trim`), `sync_anchors`, `debug_check`; report `{ scrolled: None, trimmed, history_rows: 1 }`. Carries the `hotpath-profiling` measure attribute `scroll_up` has, so profiling builds still time line feeds |
| `Anchors.lowest_follower: RowId` (new, private) | a lower bound of the rows of live anchors that are not screen-owned; `RowId(u64::MAX)` when there is none (a hand-written `Default`). `register` and `set` lower it; the walks of `trim` and `shift_region` recompute it exactly |
| `Anchors::trim` | return at once when `oldest <= lowest_follower`; else walk as today, then store the new lowest |
| `Anchors::shift_region` | return at once when `lowest_follower >= max(rows.end, kill.end)`; else walk as today (screen-owned skipped, as now), then store the new lowest |

- Same screen, report, events and counters as `scroll_up(region, 1)` for that case (both
  screens: a whole-screen region feeds history on the alternate screen too).
- The watermark is only ever too low, never too high: `register`, `set` and the two walks
  are the only writers of an anchor row (reflow moves anchors through `set`), and `release`
  or `kill` may leave it low, which only costs a walk.
- `trim` no longer kills a screen-owned anchor below `oldest` when it returns early. None can
  be there (cursor rows are on screen, the viewport top is at most `offset` rows above it
  and `offset <= history_len`), and every caller writes the screen-owned anchors back
  (`sync_anchors`, which revives) right after. `shift_region` already skipped them. One unit
  test planted a cursor anchor below `oldest`; tests.md changes it.
- Why a watermark and not a count: with images on screen (`sixel`: 256 live placements) a
  count never reaches zero and every line feed walked 262 entries (`Screen::linefeed` 15 % of
  `sixel` with a count); placements sit on screen rows, far above `oldest`.

## S6 root cause: the region rotation moves 48-byte slots (`scroll_region*`)

After 0018, a region scroll is one `slice::rotate_left` over the region's slots:
`Option<Row>` is 48 bytes, so a 43-row region moves 2 KiB per line feed (`VCRUNTIME140`
`memmove` 17 % of `scroll_region`, 14 % of `scroll_region_small`). Ghostty rotates 8-byte row
handles (`fastmem.rotateOnce`); 0018 named smaller slot entries as the next step.

## S6 design (`grid/row.rs`, `grid/screen.rs`, `grid/screen/reflow.rs`, `grid/terminal_grid.rs`)

| Item | Change |
|---|---|
| ring slots, `spare`, reflow scratch | `Option<Box<Row>>` instead of `Option<Row>` (8 bytes, null niche) |
| `Row::recycle` | takes and returns `Box<Row>`; a spare of the right width is reset in its box |
| `Screen::stored`, `slot_row_mut`, `materialise` | `as_deref()` / `as_deref_mut()`; `Row::bytes` / `into_cells` called through closures |
| `Screen::heap_bytes` | adds `size_of::<Row>()` per allocated row |

- No public item changes (`Row` stays a public type; the ring is private).
- Memory: the ring shrinks from 48 to 8 bytes per slot (primary, 16,384 slots for 10,000
  lines: 768 KiB -> 128 KiB; 48 MiB -> 8 MiB at the 1,000,000-line maximum); every written
  row adds a 48-byte box (+4 % on a 160-column row). A full history costs about the same.
- Cost: one more dependent load per row access. `scrolling` -4 % (aligned) / -6 % (plain),
  `short_lines` -1 to -3 %: past the 5 % guard, hence an owner decision (README).
- Not tried: an in-region offset (no move at all) needs every row lookup to know the region.

## Expected (pinned MiB/s, evidence.md)

| Fixture | before (S4) | after S5 | after S6 |
|---|---:|---:|---:|
| `short_lines` | 58.6 | 65.8 | 64.6 |
| `osc_9_7` | 329 | 361 | 369 (single run) |
| `sixel` | 65.0 | 72.3 | 74.1 |
| `scroll_region_small` | 57.9 | 59.7 | 64.3 |
| `scroll_region` | 565 (single run) | 558 | 616 |

Interleaved best of 3 per pair of columns (evidence.md). S6 measured +7-14 % on
`scroll_region_small` and +6-16 % on `scroll_region` across three build pairs: the least
certain step, kept last.
