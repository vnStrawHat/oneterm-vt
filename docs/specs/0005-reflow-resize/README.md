# 0005: Reflow and resize (phase 4)

**Goal:** `TerminalGrid::resize(size, policy)`: column reflow of the primary screen over logical
lines (rows joined by `WRAPPED`), row resize of both screens, cursor and anchor tracking, root
`ResizePolicy` and `ResizeOutcome`. Allocation-light: no per-row `Vec`.

**Non-goals:** `Terminal::resize`, scrollback-limit edits, DECCOLM, tier-4 bench (phase 6);
selection invalidation (5); damage / full-repaint choice (7); placements (10).

| File | Covers |
|---|---|
| [columns.md](columns.md) | logical lines, rewrap, wide pairs, trailing blanks, fast path, `RowId` promise, cap, alt clip |
| [rows.md](rows.md) | row grow/shrink, region reset, tab stops, `Size::clamped`, released ids |
| [cursor-anchors.md](cursor-anchors.md) | point mapping, death rules, `ResizePolicy`, `ResizeOutcome`, call order |
| [algorithm.md](algorithm.md) | passes, scratch, row-buffer recycling, complexity, integrity walk |
| [tests.md](tests.md) | unit tests, proptests, bench note, `docs/user/resize.md` outline |
| [files.md](files.md) | files, `lib.rs`, grid edits, deps, commit subject |

**Public API (names fixed):** root `ResizePolicy { BottomAnchor, KeepViewportTop }` (default
`BottomAnchor`); root `ResizeOutcome { reflowed: bool, rows_trimmed: u32 }` (`#[non_exhaustive]`);
`TerminalGrid::resize(&mut self, size: Size, policy: ResizePolicy) -> ResizeOutcome`.
Nothing else becomes `pub`; `grid::*` stays the 29 items of spec 0004.

## Acceptance criteria

- [ ] Quality gate; `cargo test -p oneterm-vt` with `--no-default-features` and with
      `--features vt-paranoid`; `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features`
- [ ] Every test in [tests.md](tests.md) passes, incl. both proptests
- [ ] Every resize ends with the tier-3 integrity walk (`debug_assert`); tests `check()` after each
- [ ] `docs/user/resize.md` exists; its Rust blocks run under `cargo test --doc`
- [ ] No `unsafe`, no `unwrap`/`expect`/`panic!`/`unreachable!` outside tests; no new dependency
- [ ] Commit: `feat(reflow): column reflow, row resize and ResizePolicy for TerminalGrid` (minor)

## Decisions

1. Column change: every primary row gets a fresh id from `newest_issued + 1`. Rows-only change:
   surviving ids kept. Alternate screen: ids kept (columns.md).
2. The alternate screen is clipped/padded, never reflowed.
3. Trailing trim removes exactly `Cell::EMPTY`; styled blanks, `'\t'` cells, spacers are content.
4. On the cursor's logical line the blanks up to the cursor count as content.
5. `KeepViewportTop` = the host keeps its viewport top (conhost behind ConPTY): the cursor lands on
   the row the host re-wraps it to. Both policies: a view at the bottom stays there; a
   scrolled-back view is restated from the mapped `ViewportTop` anchor.
6. Row shrink drops blank rows below the cursor first, then pushes rows into history; the cursor
   row always stays on screen (non-blank rows drop only when it otherwise could not).
7. `SelectionStart/End` map like any content anchor; phase 5 owns invalidation (roadmap: survive).
8. `rows_trimmed` = primary rows dropped off the oldest end during the call (alt not counted).
9. `resize` opens its own batch, so every row it writes or re-homes carries a new stamp.
10. Kept memory: one scratch `Vec<Option<Row>>` in `TerminalGrid`; the point list is per call.
11. A wide glyph lost to `cols == 1` degrades to a blank and counts in `dropped_wide`.
12. Row buffers keep their capacity after a shrink (`Row::bytes` already reports capacity).
13. A cursor whose line was trimmed goes to `(newest, 0)`; the saved cursor maps like the
    cursor, pending wrap included.

## Open items (owner)

1. **Released ids.** Row shrink and the `KeepViewportTop` shift lower `newest`; spec 0004 says
   "ids only grow". Amended here: ids never rewind except a resize releasing rows below the
   cursor; a released id returns only as a stamped row (`newest_issued`, rows.md). Checked: a
   monotonic alternative needs an O(history) re-home or loses the cursor row. Confirm.
