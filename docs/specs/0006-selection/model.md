# Model: `selection.rs`

Module doc: a selection is two anchors in the grid's anchor list plus the half of the cell each
pointer event hit. Every row-moving primitive already updates anchors, so a selection follows
its text through region scrolls, scroll into history and reflow with no code of its own; when
an end's content dies, the selection resolves to nothing.

## Types

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SelectionKind { Simple, Block, Lines, Semantic }
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Side { Left, Right }                      // which half of a cell the pointer hit
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SelectionRange { pub start: Pos, pub end: Pos, pub is_block: bool }
#[derive(PartialEq, Eq, Debug)]                    // no Clone/Copy: release consumes it
pub struct Selection { kind: SelectionKind, start: AnchorId, end: AnchorId,
                       start_side: Side, end_side: Side }
```

| Kind | Rustdoc meaning |
|---|---|
| `Simple` | the cells between the two pointer positions, in reading order |
| `Block` | the rectangle with the two positions as corners |
| `Lines` | whole logical lines (rows joined by `WRAPPED`) from the first end to the last |
| `Semantic` | from the start of the word at the first end to the end of the word at the last |

`SelectionRange`: both ends **inclusive**, `start <= end` (row-major), inside the live rows of
one screen, `col < cols`; for a block also `start.col <= end.col`. It is a value: valid for the
grid state it was computed from.

## `Selection` methods (all `pub`)

| Method | Rule |
|---|---|
| `new(grid: &mut TerminalGrid, kind, pos: Pos, side: Side) -> Selection` | clamp `pos`; register `SelectionStart` and `SelectionEnd` at it; both sides = `side` |
| `update(&mut self, grid: &mut TerminalGrid, pos, side)` | clamp `pos`; `anchors.set(end, pos)` (revives a dead end); `end_side = side` |
| `release(self, grid: &mut TerminalGrid)` | `anchors.release` both ids |
| `all(grid: &mut TerminalGrid) -> Selection` | active screen: `Simple` from `(oldest, 0)` `Left` to `(newest, cols - 1)` `Right` |
| `to_range(&self, grid: &TerminalGrid, escape_chars: &str) -> Option<SelectionRange>` | below |
| `text(&self, grid, graphemes: &GraphemeArena, escape_chars) -> Option<String>` | `to_range`, then text.md; `None` iff `to_range` is `None` |
| `invalidated_by(&self, grid: &TerminalGrid, inv: Invalidation) -> bool` | invalidation.md |

- Rustdoc on `Selection`: the owner must call `release`; dropping it leaks two anchor entries
  (dead or alive) until the grid is dropped. Several selections on one grid are allowed.
- **Clamp** (`new`, `update`): `screen = grid.screen_of(pos.row)`; row into
  `screen.oldest()..=screen.newest()`, col to `cols - 1`. Keeps the grid invariant that every
  live content anchor is inside its lane's live rows (tier-3 walk).

## `to_range`

1. `a = anchors.get(start)?`, `b = anchors.get(end)?` (a dead end -> `None`).
2. `screen_of(a.row)` and `screen_of(b.row)` differ (lanes) -> `None`.
3. Clamp both cols to `cols - 1` (defensive; anchors are kept in range by the grid).
4. Per kind, with `a`/`b` carrying their sides:

| Kind | Range |
|---|---|
| `Simple` | `Simple` rule below; then wide snap |
| `Block` | `Block` rule below; no snap (text.md handles pairs per row) |
| `Semantic` | order `a`, `b` by `Pos`; expand.md; then wide snap. Never `None` |
| `Lines` | order; expand.md; then wide snap. Never `None` |

**`Simple` rule.** Order the two ends by `Pos` (an end keeps its side when swapped), then:

```text
empty  if a == b (pos and side), or same row, a.side == Right, b.side == Left, a.col + 1 == b.col
b.side == Left  and a.pos != b.pos -> b steps back one cell     // col 0: (row - 1, cols - 1)
a.side == Right and a.pos != b.pos -> a steps on one cell       // tested after b moved; last col: (row + 1, 0)
a > b -> None (defensive);  else range = a ..= b
```

Examples on one row, cells 2 and 5: `L..R` = 2..=5, `R..L` = 3..=4, `L..L` = 2..=4, `R..R` =
3..=5; same cell `L..R` or `R..L` = that cell; `(c, R)..(c + 1, L)` = empty. Across a row edge,
`(r, cols - 1, R)..(r + 1, 0, L)` keeps cell `(r, cols - 1)`: the end steps back first, onto the
start, so the start no longer steps (Decision 3).

**`Block` rule.** Rows: the two ends ordered by `Pos` as above (`a` first). Columns:

```text
empty  if a.col == b.col and same side, or a.col + 1 == b.col with a Right, b Left,
       or b.col + 1 == a.col with a Left, b Right
a.col > b.col -> swap the columns and the sides of a and b (rows stay)
b.side == Left  and a.pos != b.pos and b.col > 0 -> b.col -= 1
a.side == Right and a.pos != b.pos -> a.col += 1
a.col > b.col -> None;  else rows a.row..=b.row, cols a.col..=b.col, is_block = true
```

**Wide snap** (non-block, after expansion), reading two cells through `Screen::row`:

| End | Cell | Becomes |
|---|---|---|
| start | `WideSpacer` | `col - 1` (its glyph) |
| end | `LeadingWideSpacer` on a `WRAPPED` row below `newest` | `(row + 1, 0)`, then the next rule |
| end | `Wide` | `col + 1` (its spacer) |

So a non-block range never holds half a pair, and the glyph a leading spacer stands for is
selected with it.

## `SelectionRange`

| Method | Rule |
|---|---|
| `contains(&self, pos: Pos) -> bool` | block: rows and cols both inside; else `start <= pos <= end` |
| `contains_cell(&self, row: RowRef<'_>, col: u16) -> bool` | paints-as-selected: `contains((row.id(), col))`, or the cell is `Wide` and `col + 1` is contained, or `WideSpacer` and `col - 1` is |

`contains_cell` makes a block edge that cuts a pair paint the whole glyph, matching the text.
O(1), no allocation.

## Grid accessors (crate only)

| Item | Where | Use |
|---|---|---|
| `pub(crate) fn anchors(&self) -> &Anchors` | `TerminalGrid` | `to_range`, `invalidated_by` |
| `pub(crate) fn anchors_mut(&mut self) -> &mut Anchors` | `TerminalGrid` | `new`, `update`, `release` (`register`, `set`, `release` are already crate) |
| `pub(crate) fn tabs(&self) -> &TabStops` | `Screen` | text.md tab rule (`is_stop` is already crate) |

Existing crate `screen()` / `screen_of(id)` and pub `alt_active()`, `Screen::{row, cursor,
oldest, newest, screen_top, cols, kind}` cover the rest. Nothing in `grid` becomes `pub`.
