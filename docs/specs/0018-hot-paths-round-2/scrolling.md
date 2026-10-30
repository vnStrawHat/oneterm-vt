# S7 deferred row restamp, S8 in-place alternate-screen region (scrolling)

## S7 root cause: every region scroll rewrites every region row's header

- After 0017, `rotate` moves rows as one slice, then rewrites `id` and `seq` of every row of
  the window (43 rows per LF in `scroll_region`, 24 in `scroll_region_small`).
- Ablation (restamp loop removed): `scroll_region` 358 -> 612 MiB/s, `scroll_region_small`
  26 -> 37. ~1.25 ns per row: three stores into a 48-byte slot. A tighter loop (verify pass,
  then a store-only pass) measured 0 %: the per-row touch itself is the cost.
- Ghostty rotates 8-byte row handles and marks the whole page dirty once
  (`cursorScrollRegionUp`, `fastmem.rotateOnce`). Here the stamp is per row because snapshots
  copy rows by `seq`; the equivalent is to stamp the moved range once per batch.

## S7 design

New invariant, now load-bearing (vt-paranoid's stale-slot walk already enforces it): the slot
of a live id holds nothing or that id's row. Then a header id is redundant inside a range
known to be moved, and can be rewritten later.

| Item | Change |
|---|---|
| `Screen.moved: Range<RowId>` (new, private) | hull of the windows rotated in this batch; empty after `restamp_moved` |
| `Screen::rotate` (slice path) | rotate the slice; write only the vacated rows (`rehome` + `reset(erase)`; an empty vacated slot stays empty when `erase` is the plain blank, else built from the spare); then widen `moved` by `lo..lo + len`. No pass over the moved rows |
| `Screen::restamp_moved` (new, `pub(crate)`) | for live ids in `moved`: `materialise(id)` (an empty slot becomes a blank row), then `rehome(id, seq)`; clear `moved` |
| callers of `restamp_moved` | `set_seq` (before the new stamp: every batch starts settled), `rotate_by_swaps`, `set_scrollback_limit`, `Screen::reset` (resize and reflow run after `begin_batch`) |
| `Screen::stored` / `materialise` | a slot's row counts as `id`'s when its header says `id` or `id` is in `moved`; compare the header first and test the range only on a mismatch (an eager range test cost `cursor_motion` 5 %) |
| `Screen::row` | in `moved`: `RowRef::moved(row, id, seq)`; an empty slot in `moved`: `RowRef::blank(..).moved_at(seq)` |
| `RowRef` | private `moved_at: SeqNo`; `seq()` = max(header stamp, `moved_at`); `flags()` of a moved blank reports `DIRTY`; `id` comes from the caller (already a field) |
| `check_rows`, `check_stale_slots` | skip the header-id compare for ids in `moved` (the stale-slot walk derives a slot's live id from `newest` and the mask) |

- Readers see exactly today's ids, stamps and cells through `row`, `row_mut`, `row_text`,
  snapshots, selection and search: only the stored headers lag, within one batch.
- `rotate_by_swaps` (window across the ring end) restamps first, then runs as today.
- `slice_rotation_never_revives_a_stale_row` plants a state the invariant excludes; it is
  replaced by an integrity test (tests.md).
- Rejected: building empty slots of the window before rotating (keeps blanks out of `moved`):
  `scroll_region` 474 -> 397, `scroll_region_small` 33.7 -> 28.2.

## S8 root cause: the alternate screen scrolls a top region through a history it drops

`scroll_region_small` (alternate screen, `CSI 1;22 r`): every LF pushes a row, trims it at
once (no history on that screen), and rotates the 24 rows below the region down; two events
per LF. Ghostty: "creating scrollback is pure overhead ... use the in-place region scroll".

## S8 design (owner decision, README)

`Screen::scroll_up`: a region with `top == 0` takes `scroll_up_into_history` only when
`kind == Primary` or the region is the whole screen (`bottom == rows`). A partial region on
the alternate screen rotates in place like a region with `top > 0`, and its report keeps
`history_rows: n`: the rows still "left the top of a screen" and are dropped at once, which is
what `rows_scrolled` and `lines_produced` already say for that screen. No counter changes
meaning (versioning.md: that would be a major bump).

| Observable on the alternate screen, `top == 0`, `bottom < rows` | Before | After |
|---|---|---|
| events per scroll | `RowsScrolled` (rows below the region, +n), `RowsTrimmed` | `RowsScrolled` (region rows, -n) |
| alternate row ids | advance by n per scroll | unchanged |
| `rows_scrolled`, `lines_produced` | +n | +n (unchanged) |
| cells, cursor, anchors on content | | unchanged (all 23 dumps equal) |

`docs/user/events.md`, `RowsScrolled` / `RowsTrimmed` row: add "a partial region at the top
of the alternate screen moves its rows in place: `RowsScrolled`, no `RowsTrimmed`".
`docs/user/limits.md` stays as it is.

## Expected (pinned MiB/s)

| Fixture | before (S6) | after S7 | after S8 |
|---|---:|---:|---:|
| `scroll_region` | 343 (best of 3: 342) | 539 (562) | 578 (571) |
| `scroll_region_small` | 26.3 (26.2) | 33.0 (33.9) | 58.5 (59.1) |
