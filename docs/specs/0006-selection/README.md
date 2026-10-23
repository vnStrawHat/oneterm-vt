# 0006: Selection (phase 5)

**Goal:** root `Selection`, `SelectionKind`, `SelectionRange`, `Side`, `Invalidation`: two-anchor
selections that follow scrolls and reflow; simple, block, word, line kinds; text; erase rules.

**Non-goals:** `Terminal`, `Config`, the `selection_*` wrappers, `hit_test`, `row_text` on
`Terminal`, the dispatcher calls (phase 6, listed in [invalidation.md](invalidation.md));
painting and snapshot fields (7); search (8). Builds on phase 4 (`TerminalGrid::resize`) as landed.

| File | Covers |
|---|---|
| [model.md](model.md) | types, `Selection` methods, input clamp, `to_range` per kind, `Side`, wide snap, `contains*`, grid accessors |
| [expand.md](expand.md) | escape set, word search, bracket match, line walk, scan budget, block rules, `Side` per kind |
| [text.md](text.md) | per-row text, trim, tabs, graphemes, newline rules, `Lines` newline, `Terminal::row_text` |
| [invalidation.md](invalidation.md) | `Invalidation` rules, row-moving ops and anchor death, what phase 6 calls, `hit_test` |
| [tests.md](tests.md) | unit tests, proptests, `docs/user/selection.md` outline |
| [files.md](files.md) | files, grid edits, `lib.rs`, deps, commit subject |

**Public API (names fixed):** root `Invalidation, Selection, SelectionKind, SelectionRange, Side`;
`Selection::{new, update, release, all, to_range, invalidated_by, text}`;
`SelectionRange::{contains, contains_cell}` + pub fields `start, end, is_block`. Nothing else `pub`.

## Acceptance criteria

- [ ] Quality gate; `cargo test -p oneterm-vt` with `--no-default-features` and `--features vt-paranoid`;
      `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features`
- [ ] Every test in [tests.md](tests.md) passes, incl. the three proptests
- [ ] Roadmap: selections survive scroll and reflow; word/line expansion; text extraction (tests)
- [ ] `docs/user/selection.md` exists; its Rust blocks run under `cargo test --doc`
- [ ] No `unsafe`, no `unwrap`/`expect`/`panic!`/`unreachable!` outside tests; no new dependency
- [ ] Commit: `feat(selection): anchored selections with word, line and block modes` (minor)

## Decisions

1. Both ends are anchors (`SelectionStart`, `SelectionEnd`) in the grid's list; a dead end = no range.
2. A column reflow maps the ends like any content anchor: the selection survives (0005 Decision 7).
3. Classic `Side` rules for `Simple` and `Block`: the end steps back before the start steps on, so
   `(r, last, R)..(r + 1, 0, L)` keeps cell `(r, last)`; the same drag inside one row is empty.
4. A wide pair is atomic: non-block ranges snap outward; blocks handle pairs per row.
5. Default escape set `` ,│`|:"' ()[]{}<>\t ``; an endpoint's own cell is not tested (expand.md).
6. Coincident `Semantic` ends on a bracket select to its match, `()[]{}<>`, nesting counted.
7. Scans capped at `EXPAND_BUDGET = 65_536` cells (rows for `Lines`) per direction; `to_range` is interner- and allocation-free.
8. Text trims blank (`' '`) cells past a row's content end, styled or not; `WRAPPED` rows are never trimmed.
9. `Lines` text ends with exactly one `'\n'`; other kinds never end with one.
10. A `'\t'` cell plus the blanks after it up to the next stop copy as one `'\t'`.
11. Erase rules are row-level, asked before the op, against the selection's own screen and cursor;
    `EraseScreen` (every ED 2), `SwapAlt`, `Reset` and a dead end always invalidate.
12. `Selection` is not `Clone`/`Copy`: `release(self)` gives both anchors back exactly once.
13. Positions clamp into their lane's live rows; ends in different lanes resolve to `None`.
14. `text` takes `&GraphemeArena` (like `Screen::row_text`); `contains_cell(RowRef, col)` is
    pair-aware and takes no cursor (cursor drawing is the renderer's).
15. A region scroll moving one end and not the other stretches the selection (each end follows its content).
16. Grid access is crate-only: `TerminalGrid::{anchors, anchors_mut}`, `Screen::tabs` (files.md).

## Open items

None.
