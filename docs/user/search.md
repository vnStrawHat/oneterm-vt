# Search

Search is two phases. `GridText::from_terminal` copies the active screen's text (history and
screen) while you hold your lock; `search_grid_text` matches the copy without it, on any thread.
A match names `RowId`s, so it stays correct while output scrolls. Nothing returns an error.

## Example: find a word

```rust
use std::time::Instant;
use oneterm_vt::search::{GridText, SearchOptions, SearchPattern, search_grid_text};
use oneterm_vt::{Config, EventBatch, Size, Terminal};

let mut term = Terminal::new(Size { rows: 4, cols: 20 }, Config::default());
term.feed(b"Hello World", &mut EventBatch::new(), Instant::now());

let text = GridText::from_terminal(&term); // under your lock; the copy needs none
let found = search_grid_text(&text, SearchPattern::Literal("world"), SearchOptions::default());
assert_eq!(found.len(), 1);
assert_eq!((found[0].start_col, found[0].end_col), (6, 11));
```

## What a match means

- `row` / `start_col` is the first cell; `end_row` / `end_col` is the **exclusive** end. A match
  ending on a row's last cell has `end_col == cols`; one spanning a wrap has `end_row != row`, so
  paint the first row from `start_col`, the middle rows whole, the last row up to `end_col`.
- `grid_line(screen_top)` is the start row relative to the top screen row (`screen().screen_top()`):
  negative is history. `display_row(screen_top, offset)` adds the viewport offset
  (`screen().scroll_offset() as usize`); both saturate; dropping rows outside `0..rows` is yours.

## Wrapped lines

Rows joined by a soft wrap form one logical line, so a word the wrap split is found. A hard line
end (`\r\n`, `\n` after a short row) never joins. An unwrapped row ends at its last non-blank cell:
trailing blanks are not text, and a needle holding `'\n'` or nothing finds nothing.

```rust
use std::time::Instant;
use oneterm_vt::search::{GridText, SearchOptions, SearchPattern, search_grid_text};
use oneterm_vt::{Config, EventBatch, Size, Terminal};

let mut term = Terminal::new(Size { rows: 3, cols: 10 }, Config::default());
term.feed(b"0123456789abc", &mut EventBatch::new(), Instant::now());

let text = GridText::from_terminal(&term);
let found = search_grid_text(&text, SearchPattern::Literal("89ab"), SearchOptions::default());
assert_eq!((found[0].start_col, found[0].end_col), (8, 2));
assert_eq!(found[0].end_row, found[0].row + 1); // ends on the next row
```

## Literal options

`SearchOptions` is `#[non_exhaustive]`: start from `default()` and set fields.

- `case_sensitive` (default `false`): without it each char is lower-cased 1:1 (ASCII, Latin, Greek,
  Cyrillic, Armenian), not Unicode-folded: `ß` vs `ss`, `ς`/`σ`, `ſ`/`s`, `İ`/`i` and the Kelvin sign vs `k` stay distinct.
- `whole_word`: a hit is rejected when it sits inside a word at its start or its end. Word chars
  are Unicode letters, digits and `_`.

```rust
use std::time::Instant;
use oneterm_vt::search::{GridText, SearchOptions, SearchPattern, search_grid_text};
use oneterm_vt::{Config, EventBatch, Size, Terminal};

let mut term = Terminal::new(Size { rows: 3, cols: 20 }, Config::default());
term.feed(b"cargo cargofmt", &mut EventBatch::new(), Instant::now());
let text = GridText::from_terminal(&term);

let mut options = SearchOptions::default();
let cargo = SearchPattern::Literal("cargo");
assert_eq!(search_grid_text(&text, cargo, options).len(), 2);
options.whole_word = true;
assert_eq!(search_grid_text(&text, cargo, options).len(), 1);
```

## Limits and cost

- At most `MAX_MATCHES` (100 000) matches come back; a result of exactly that length means there
  may be more. Narrow the pattern or search again.
- `from_terminal` is one pass and holds about one byte per ASCII cell up to each row's content end
  (100 000 rows of 200 columns, half full, is about 13 MB), more for wide glyphs and clusters.
  History length is your `scrollback_limit`; the copy has no cap of its own.

## A live grid

A `GridText` is the grid as it stood. Matches stay valid while their rows are live. Drop or move
them on `VtEvent::RowsTrimmed { oldest }` (rows older than `oldest` are gone) and after a column
resize that reflows (`ResizeOutcome::reflowed`): primary rows get fresh ids, so copy and search
again. The alternate screen has no history; search it while it is active.

## Regex (feature `regex`)

Enable it with `oneterm-vt = { version = "..", features = ["regex"] }`. It adds
`SearchPattern::Regex(&regex::Regex)`: you compile the pattern (flags, size limit), the crate runs
it over each logical line. `^` and `$` anchor to the line's first char and its content end;
`SearchOptions` is ignored, so write `(?i)` and `\b` in the pattern. A pattern that matches the
empty string yields one empty match per char boundary. The block compiles without the feature
(`main` is empty there).

```rust
#[cfg(feature = "regex")]
fn main() {
    use std::time::Instant;
    use oneterm_vt::search::{GridText, SearchOptions, SearchPattern, search_grid_text};
    use oneterm_vt::{Config, EventBatch, Size, Terminal};
    let mut term = Terminal::new(Size { rows: 3, cols: 30 }, Config::default());
    term.feed(b"error 404 not found", &mut EventBatch::new(), Instant::now());
    let text = GridText::from_terminal(&term);

    let regex = regex::Regex::new(r"error\s+\d+").expect("a valid pattern");
    let found = search_grid_text(&text, SearchPattern::Regex(&regex), SearchOptions::default());
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].start_col, found[0].end_col), (0, 9));
}
#[cfg(not(feature = "regex"))]
fn main() {}
```

`SearchPattern` is `#[non_exhaustive]`: a `match` on it needs a wildcard arm.
