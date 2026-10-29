# Items 3 and 5: scrolling (S5, S6)

## S5 root cause: per-line cost into history (`short_lines`, the CR LF part of every text fixture)

- With the history full, every LF pushes one `RowsTrimmed` (32-byte `VtEvent`): 21,845 per
  64 KiB feed of `short_lines`. `EventBatch::clear` shrinks the list to `EVENT_LIST_SOFT` =
  4,096 at the start of every feed, so each feed regrows it 4,096 -> 32,768 (three reallocations,
  ~0.9 MB copied, fresh pages). Prototype: no shrink 51 -> 58 MiB/s; no scroll events at all 61.
- `Row::recycle` rebuilds the trimmed spare by `clear` + `resize(cols)`: all 160 cells are
  rewritten although the `occ` invariant says only the first `occ` can differ.
- Coalescing consecutive scroll events was rejected: `Terminal::feed` promises the same
  events for any chunking (`tests/feed_chunking.rs` compares them).

## S5 design

| Item | Change |
|---|---|
| `EVENT_LIST_SOFT` | 4,096 -> 32,768 events (1 MiB at 32 B, the budget `EVENT_ARENA_SOFT` already has) |
| `PARAM_LIST_SOFT` | new constant, 4,096: the OSC parameter-span list keeps today's cap |
| `EventBatch` doc, `docs/user/limits.md` "event arena and lists" row | "1 MiB of payload, 32,768 events or 4,096 parameter spans" |
| `Row::recycle` | a spare with `cells.len() == cols` is reused through `Row::reset(template, seq)` (occ-aware) plus the new id; any other spare keeps the clear + resize path |

One hostile feed still cannot pin its peak: anything above the caps is released by the next
`clear`, as today.

## S6 root cause: region scrolls walk the ring three times

`Screen::rotate` already moves rows, not cells, but per moved row it calls `materialise`
(slot index + id compare), `swap_slots` (two slot indexes, bounds checks, a 48-byte swap) and
`slot_row_mut` + `rehome`: three ring-indexed passes over `len` rows. `scroll_region` rotates
the 43-row region per LF; `scroll_region_small` (alternate screen, region rows 1-22) goes
through `scroll_up_into_history`, which pushes one row and rotates the 24 rows below the
region down. Profile: `rotate` 28 % of `scroll_region`, 25 % of `scroll_region_small`.

## S6 design

```rust
fn rotate(&mut self, lo: RowId, len: u64, shift: Shift, n: u64) {
    let start = self.slot_index(lo);
    // A window that wraps the ring end (at most once per ring cycle) keeps the swap loop.
    if start + len as usize > self.slots.len() { return self.rotate_by_swaps(lo, len, shift, n); }
    // 1. materialise every row of the window (one pass, ids lo..lo+len)
    // 2. window.rotate_left(n) for Shift::Up, window.rotate_right(n) for Shift::Down
    // 3. one pass: rehome(lo + k, seq); the vacated rows also reset(erase, seq)
}
```

- Same result as today, row for row: ids, `seq` stamps, `DIRTY`, the erase fill of the vacated
  rows, the spare consumed by `materialise`. `rotate_by_swaps` is today's body, unchanged.
- `n` is clamped to `len` before the slice rotation (callers already clamp; keep it local).
- Rejected: rotating the alternate screen's top-anchored region in place instead of pushing a
  row (2 rows less work per LF) changes `RowsTrimmed`/`RowsScrolled` events and the
  `rows_scrolled` counter documented in limits.md.

## Expected (pinned MiB/s, prototype)

| Fixture | HEAD | after S5 | after S6 (with S2) |
|---|---:|---:|---:|
| `short_lines` | 38 | 58 | 55-60 |
| `scroll_region` | 91 | - | 271-288 |
| `scroll_region_small` | 10.5 | 17 | 26-27 |
| `plain_ascii` (recycle only, after S2) | 408 | 455 | - |

`plain_ascii`'s recycle gain is within the spread; the change is kept because it is local and
removes 1.2 KiB of writes per scrolled line.

## Risk and protection

- Snapshots copy rows by `seq`: the rotation must stamp every moved row (step 3). Existing
  snapshot and `grid_props` tests check stamps after random operations; tests.md adds a ring
  wrap case.
- The swap fallback is reached only when the window crosses the ring end: tests.md forces it
  with a scrollback of 0 (1,024-slot ring) and enough line feeds to wrap.
