# Grid

`oneterm_vt::grid::TerminalGrid` holds the two screens of a terminal: rows of `Cell`s in a
ring-buffer scrollback, a cursor, tab stops and a scroll region. Rows are addressed by `RowId`, a
position in the output stream. Nothing here returns an error or panics: counts clamp, and a
row never written reads as blanks.

## Example: write a row and scroll it into history

```rust
use oneterm_vt::grid::TerminalGrid;
use oneterm_vt::intern::Interner;
use oneterm_vt::{Cell, CellContent, Size};

let mut grid = TerminalGrid::new(Size { rows: 3, cols: 10 }, 100);
let interner = Interner::default();

let screen = grid.screen_mut();
let id = screen.cursor().pos.row;
let mut row = screen.row_mut(id).expect("the cursor row is live");
for (col, c) in "hello".chars().enumerate() {
    row.set(col as u16, Cell::EMPTY.with_content(CellContent::Scalar(c)));
}
for _ in 0..3 {
    grid.wrapline(); // the last one scrolls the top row into history
}

let screen = grid.primary();
assert_eq!(screen.history_len(), 1);
let mut text = String::new();
screen.row_text(screen.oldest(), &interner.graphemes, &mut text);
assert!(text.starts_with("hello"));
assert!(screen.row(screen.oldest()).wrapped());
assert!(!screen.row(screen.newest()).is_allocated()); // never written: costs nothing
```

## Screen and viewport

- The **screen** is the bottom `rows` rows; every edit addresses it by index. The **viewport**
  is what the user sees: `scroll_offset()` rows above the screen. Scrolling back never moves
  the cursor, and new output does not drag a scrolled-back view along.
- The primary screen keeps scrollback; the alternate screen (`alt()`, `alt_active()`) keeps none.

```rust
use oneterm_vt::grid::TerminalGrid;
use oneterm_vt::Size;

let mut grid = TerminalGrid::new(Size { rows: 3, cols: 10 }, 100);
for _ in 0..10 {
    grid.wrapline();
}
let screen = grid.screen_mut();
let live_top = screen.visible_top();
screen.scroll_viewport(-3); // negative: towards history; clamps to history_len()
assert_eq!(screen.scroll_offset(), 3);
assert_eq!(screen.visible_top(), live_top - 3);
screen.scroll_to_bottom();
assert_eq!(screen.visible_top(), live_top);
assert!(!grid.alt_active() && grid.alt().history_len() == 0);
```

## Rows and ids

- A `RowId` names a position in the stream, not a piece of content. Ids never rewind and are
  never reset by a clear or a reset, except that a resize may release rows below the cursor; a
  released id comes back only as a freshly stamped row (see [resize.md](resize.md)). The primary
  screen counts from `RowId::PRIMARY_ORIGIN`, the alternate from `RowId::ALT_ORIGIN`, so the two
  lanes never meet.
- A scroll of the whole screen moves no content; a scroll of a region does, and content then
  changes id. Follow content with `Anchors`, never by keeping a `RowId` across edits.
- Read a row with `Screen::row(id)`, a `RowRef`: `cells()`, `cell(col)` (blank past the end),
  `wrapped()`, `is_allocated()`. Write only through `RowMut`: it keeps double-width pairs
  well formed, so a `Wide` cell is always followed by its `WideSpacer`.

## Scrollback and memory

```rust
use oneterm_vt::grid::{DEFAULT_SCROLLBACK, SCROLLBACK_MAX, TerminalGrid};
use oneterm_vt::Size;

let grid = TerminalGrid::new(Size { rows: 24, cols: 80 }, DEFAULT_SCROLLBACK);
assert_eq!(DEFAULT_SCROLLBACK, 10_000);
assert_eq!(SCROLLBACK_MAX, 1_000_000);
assert_eq!(grid.primary().ring_len(), 16_384); // a power of two, fixed for the grid's life
assert_eq!(grid.alt().ring_len(), 1_024);
assert!(grid.heap_bytes() < 1_000_000); // an empty scrollback costs only its ring
```

- The ring has `(scrollback + 1024).next_power_of_two()` slots of 48 bytes. A slot is filled
  only when its row is first written, so unwritten history costs nothing beyond the ring.
- `oldest()`, `newest()`, `row_range()` and `history_len()` describe the live ids. Rows older
  than `oldest()` are gone; asking for one gives blanks.
- Sizes are clamped to 1..=1024 rows and 1..=2048 columns, scrollback to `SCROLLBACK_MAX`.

## Writing cells into a terminal

A test fixture, a replay tool or a viewport controller can write rows of a live `Terminal`
through `grid_mut()` and `interner_mut()`. Writes bypass the parser, so the selection, image
placements and colour overrides are not told.

- Call `grid_mut().begin_batch()` first: a `SnapshotState` that already saw the current batch
  skips the row otherwise.
- Mint style, cluster and link ids with `interner_mut()` and write the cell in the same step,
  before the next `feed` (a feed sweeps ids no row uses).
- `row_mut(id).set(col, cell)` keeps wide pairs whole: a `Wide` cell writes its own spacer, so
  never write a spacer by hand. `set_wrapped(true)` joins the row to the next.
- Moving the viewport by hand is `scroll_viewport(d)` or `scroll_to_bottom()` on `screen_mut()`,
  then `grid_mut().sync_anchors()`; `Terminal::scroll_viewport` does both.

```rust
use std::time::Instant;
use oneterm_vt::{
    Cell, CellContent, CellWidth, Config, Extras, Size, SnapshotContent, SnapshotState, Terminal,
};

let mut term = Terminal::new(Size { rows: 3, cols: 8 }, Config::default());
let (mut state, now) = (SnapshotState::new(), Instant::now());
term.snapshot_update(&mut state, now);

let interner = term.interner_mut();
let link = interner.hyperlinks.intern(None, "https://example.com");
let extras = interner.extras(Extras { hyperlink: link, graphic: None });
let wide = Cell::EMPTY
    .with_content(CellContent::Scalar('\u{4e2d}'))
    .with_width(CellWidth::Wide);
let linked = Cell::EMPTY.with_content(CellContent::Scalar('a')).with_extras(extras);

let id = term.screen().screen_top();
term.grid_mut().begin_batch();
let mut row = term.grid_mut().screen_mut().row_mut(id).expect("the screen row is live");
row.set(0, wide); // also writes the spacer in column 1
row.set(2, linked);
term.snapshot_update(&mut state, now);

let row = &state.rows()[0];
assert_eq!(row.cells[0].content, SnapshotContent::Scalar('\u{4e2d}'));
assert!(row.cells[1].width.is_spacer());
let link = row.cells[2].hyperlink.expect("the cell carries its link");
assert_eq!(&*state.hyperlink(link).expect("copied with the row").uri, "https://example.com");
```

## Reading checklist

- Resolve `style_id()`, `extras_id()` and grapheme ids through the terminal's `Interner`.
- Skip spacer cells (`CellWidth::is_spacer`) when drawing or extracting text; `row_text` does.
- A `Wide` cell holds the glyph and spans two columns; its spacer holds nothing.
- `SeqNo` is a change stamp per batch of input, not per edit: a row changed in a batch carries
  that batch's stamp, so comparing one number per row finds the changed rows.
