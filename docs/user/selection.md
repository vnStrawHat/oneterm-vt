# Selection

`oneterm_vt::Selection` is a text selection that follows its content. It holds two anchors in the
grid's anchor list, so a selection survives scrolls into history and column reflow, and resolves
to nothing once the text it marked is gone. `to_range` gives the cells, `text` the copied string.
Nothing here returns an error: out-of-range positions clamp, and a selection with no cells gives
`None`.

## Example: select a word and copy it

```rust
use oneterm_vt::grid::TerminalGrid;
use oneterm_vt::intern::Interner;
use oneterm_vt::{Cell, CellContent, Pos, Selection, SelectionKind, Side, Size};

let mut grid = TerminalGrid::new(Size { rows: 3, cols: 20 }, 100);
let id = grid.screen_mut().row_of_index(0);
{
    let mut row = grid.screen_mut().row_mut(id).expect("a screen row is live");
    for (col, c) in "hello world".chars().enumerate() {
        row.set(col as u16, Cell::EMPTY.with_content(CellContent::Scalar(c)));
    }
}

let interner = Interner::default();
let delimiters = " ,;:\"'()[]{}<>\t"; // what ends a word is your policy
let at_w = Pos { row: id, col: 6 };
let selection = Selection::new(&mut grid, SelectionKind::Semantic, at_w, Side::Left);
let text = selection.text(&grid, &interner.graphemes, delimiters);
assert_eq!(text.as_deref(), Some("world"));
selection.release(&mut grid); // always: dropping would leak two anchors
```

For a drag, call `Selection::update(&mut grid, pos, side)` as the pointer moves.
`Selection::all` selects the active screen with its history.

## Kinds and `Side`

| `SelectionKind` | Selects |
|---|---|
| `Simple` | the cells between the two pointer positions, in reading order |
| `Block` | the rectangle with the two positions as corners |
| `Lines` | whole logical lines (rows joined by the wrap flag) |
| `Semantic` | the word at each end; a click on a bracket selects through its match |

`Side` says which half of a cell the pointer hit. For `Simple` and `Block`, `Left` includes the
cell at the start of the drag and excludes it at the end; `Right` is the reverse. A click with no
drag, or two halves with no whole cell between them, selects nothing (`to_range` is `None`).
`Semantic` and `Lines` ignore `Side` and are never empty. A double-width glyph is atomic: a
non-block range never holds half of one, and a block edge that cuts one still copies and paints
the whole glyph (`SelectionRange::contains_cell`).

`SelectionRange` has inclusive `start` and `end` and `is_block`; it is valid for the grid state it
was computed from. Words and lines longer than 65,536 cells (rows) are cut there.

## Copied text

- Rows joined by the wrap flag copy without a newline; any other row break copies as `'\n'`.
- Trailing blanks of a line are dropped, styled or not. A `Block` trims each row's slice.
- A tab and the blanks it skipped copy as one `'\t'`.
- Clusters are resolved through the `GraphemeArena` you pass (`Interner::graphemes`).
- `Lines` text ends with one `'\n'`; no other kind's does. Blanks only gives `Some("")`.

## Following content

```rust
use oneterm_vt::grid::TerminalGrid;
use oneterm_vt::intern::Interner;
use oneterm_vt::{Cell, CellContent, Pos, ResizePolicy, Selection, SelectionKind, Side, Size};

let mut grid = TerminalGrid::new(Size { rows: 4, cols: 10 }, 100);
let first = grid.screen_mut().row_of_index(0);
for (index, text) in ["abcdefghij", "kl"].into_iter().enumerate() {
    let id = grid.screen_mut().row_of_index(index as u16);
    let mut row = grid.screen_mut().row_mut(id).expect("a screen row is live");
    for (col, c) in text.chars().enumerate() {
        row.set(col as u16, Cell::EMPTY.with_content(CellContent::Scalar(c)));
    }
    row.set_wrapped(index == 0); // the first row continues on the second
}

let interner = Interner::default();
let start = Pos { row: first, col: 2 };
let mut selection = Selection::new(&mut grid, SelectionKind::Simple, start, Side::Left);
let end = Pos { row: first + 1, col: 1 };
selection.update(&mut grid, end, Side::Right);
let copy = |grid: &TerminalGrid| selection.text(grid, &interner.graphemes, "");
assert_eq!(copy(&grid).as_deref(), Some("cdefghijkl"));

grid.resize(Size { rows: 4, cols: 4 }, ResizePolicy::BottomAnchor); // rewraps
assert_eq!(copy(&grid).as_deref(), Some("cdefghijkl"));
selection.release(&mut grid);
```

- Scrolling the screen into history, scrolling the viewport and reflow keep the selection.
- A region scroll moves each end with its row; an end whose row scrolls out of the region, is
  trimmed from the history or is wiped dies, and `to_range` is `None` until `update` revives it.
- Resizing the alternate screen keeps rows and clamps columns.

## Embedder checklist

- Erase operations blank cells in place and keep row ids, so ask `Selection::invalidated_by(&grid, inv)`
  **before** running one, and `release` the selection on `true`:

  | VT operation | `Invalidation` |
  |---|---|
  | EL 0, 1, 2 | `EraseLine` |
  | ED 0 / 1 / 2 / 3 | `EraseBelow` / `EraseAbove` / `EraseScreen` / `EraseHistory` |
  | DECALN, DECCOLM | `EraseScreen` |
  | switching to or from the alternate screen | `SwapAlt` |
  | RIS | `Reset` |

- A selection with a dead end reports `true` for every `Invalidation`.
- ECH, DCH, ICH and printing over selected cells do not invalidate: the selection copies what the
  cells hold now.
- Always call `release`; a selection holds two anchors until you do.
- Pass the word delimiters you want as `escape_chars`; a cell holding a cluster is never one.
