# Resize

`oneterm_vt::grid::TerminalGrid::resize(size, policy)` changes the window size. A column change
re-lays the primary screen out over its **logical lines** (rows joined by the wrap flag); a row
change pulls history in or pushes rows out. Cursors, the viewport and anchors follow their content.
Nothing here returns an error: sizes clamp to 1..=1024 rows and 1..=2048 columns, and a resize to
the current size does nothing.

## Example: a wrapped line at a new width

```rust
use oneterm_vt::grid::TerminalGrid;
use oneterm_vt::intern::Interner;
use oneterm_vt::{Cell, CellContent, ResizePolicy, Size};

let mut grid = TerminalGrid::new(Size { rows: 3, cols: 10 }, 100);
let screen = grid.screen_mut();
for (index, text) in ["abcdefghij", "kl"].into_iter().enumerate() {
    let id = screen.row_of_index(index as u16);
    let mut row = screen.row_mut(id).expect("a screen row is live");
    for (col, c) in text.chars().enumerate() {
        row.set(col as u16, Cell::EMPTY.with_content(CellContent::Scalar(c)));
    }
    row.set_wrapped(index == 0); // the first row continues on the second
}

let before = grid.primary().newest();
let outcome = grid.resize(Size { rows: 3, cols: 5 }, ResizePolicy::BottomAnchor);
assert!(outcome.reflowed);
assert_eq!(outcome.rows_trimmed, 0);

let interner = Interner::default();
let screen = grid.primary();
let mut texts = Vec::new();
let mut id = screen.oldest();
while id <= screen.newest() {
    let mut text = String::new();
    screen.row_text(id, &interner.graphemes, &mut text);
    texts.push(text);
    id = id + 1;
}
assert_eq!(texts, ["abcde", "fghij", "kl   "]);
assert!(screen.row(screen.oldest()).wrapped());
assert!(screen.oldest() > before); // a column change gives every row a fresh id
```

## Choosing a policy

| `ResizePolicy` | Behaviour | Use for |
|---|---|---|
| `BottomAnchor` (default) | the bottom of the content stays at the bottom: growing pulls history in at the top, shrinking pushes rows into it | PTYs that redraw after the resize signal (Unix, SSH) |
| `KeepViewportTop` | the top screen row stays put and the cursor lands on the row the host re-wraps it to; new rows appear blank at the bottom | local Windows shells behind ConPTY |

```rust
use oneterm_vt::grid::TerminalGrid;
use oneterm_vt::{ResizePolicy, Size};

let scrolled = || {
    let mut grid = TerminalGrid::new(Size { rows: 3, cols: 10 }, 100);
    for _ in 0..6 {
        grid.wrapline(); // the last ones scroll rows into history
    }
    grid
};
let bigger = Size { rows: 5, cols: 10 };

let mut bottom = scrolled();
bottom.resize(bigger, ResizePolicy::BottomAnchor);
assert_eq!(bottom.primary().history_len(), 2); // 4 before: two rows came back

let mut top = scrolled();
let screen_top = top.primary().screen_top();
top.resize(bigger, ResizePolicy::KeepViewportTop);
assert_eq!(top.primary().screen_top(), screen_top); // the same row is still on top
```

A viewport that follows output (`scroll_offset() == 0`) stays at the bottom under both policies.
A scrolled-back viewport keeps showing the content at its top row.

## What reflow keeps

- **Logical lines**: rows joined by the wrap flag are re-cut at the new width; a line end never
  joins its neighbour. Trailing plain blanks are trimmed; styled blanks and tabs are content.
- **Cells**: styles, hyperlinks, grapheme clusters and semantic marks are copied as they are. A
  double-width glyph never splits: the last column becomes a filler and the glyph moves down.
  At one column a wide glyph degrades to blanks and counts in `Screen::dropped_wide`.
- **The cursor** keeps its place in the text, including the blanks between the text and itself and
  a pending wrap on an exact fit. A cursor whose text scrolled out of the scrollback goes to the
  bottom row, column 0.
- **The alternate screen** is clipped or padded, never reflowed: its program redraws itself.
- **History** is limited by the scrollback limit; a narrower width makes more rows, so the oldest
  fall off.

## Ids and anchors

- A column change gives every primary row a **new id** above all old ones. A row-only change keeps
  ids. Never keep a `RowId` across a resize; follow content with anchors.
- Content anchors are mapped to the cell they were on. They die when their row falls off the oldest
  end, or is released below the cursor on a shrink. Cursor, saved cursor and viewport anchors
  never die.
- Ids never rewind, except that a shrink may release blank rows below the cursor; a released id
  comes back only as a freshly stamped row.

## `ResizeOutcome`

| Field | Meaning |
|---|---|
| `reflowed` | the column count changed, so the primary screen was re-laid out (true even if every line kept its row) |
| `rows_trimmed` | primary rows that fell off the oldest end during the call; rows released at the bottom do not count |

The struct is `#[non_exhaustive]`: read the fields, do not build it.

## Embedder checklist

- Resize the PTY (`TIOCSWINSZ` or `ResizePseudoConsole`) as well: the program redraws on the signal.
- Repaint what changed: every row a resize writes carries the new batch stamp, so a stamp-based
  refresh picks them up; after a column change that is every primary row.
- Drop selections and marks whose `Anchors::get` is `None` after `rows_trimmed > 0`.
