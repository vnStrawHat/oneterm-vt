# Model: `search.rs`

Module doc: search is two phases. `GridText::from_terminal` copies the active screen's text
(history and screen) under the embedder's lock; `search_grid_text` matches the copy without it.
A match names `RowId`s, so it stays correct while output scrolls underneath it.

## Public items (`oneterm_vt::search`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] #[non_exhaustive]
pub struct SearchOptions { pub case_sensitive: bool, pub whole_word: bool }   // both false
#[derive(Debug, Clone, Copy)] #[non_exhaustive]
pub enum SearchPattern<'a> { Literal(&'a str), #[cfg(feature = "regex")] Regex(&'a ::regex::Regex) }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch { pub row: RowId, pub start_col: usize, pub end_row: RowId, pub end_col: usize } // D1
impl SearchMatch { pub fn grid_line(&self, screen_top: RowId) -> i32;
                   pub fn display_row(&self, screen_top: RowId, display_offset: usize) -> i32; }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridText { /* private */ }
impl GridText { pub fn from_terminal(term: &Terminal) -> GridText;
                pub fn oldest(&self) -> RowId; pub fn rows(&self) -> usize; }
pub const MAX_MATCHES: usize = 100_000;                                         // D6
pub fn search_grid_text(text: &GridText, pattern: SearchPattern<'_>, options: SearchOptions) -> Vec<SearchMatch>;
```

- `SearchMatch`: `row`/`start_col` = first cell; `end_row`/`end_col` = exclusive end on
  `end_row`. One-row match: `end_row == row`. A match ending on a row's last cell has
  `end_col == cols`. An empty regex match has `end_row == row`, `end_col == start_col`, and
  `start_col` may equal the row's cell count (an empty match at the line end).
- `grid_line(top)` = `row - top` as `i32` (clamped; negative = history, `-1` newest history row).
  `display_row(top, offset)` = `grid_line(top).saturating_add(i32::try_from(offset).unwrap_or(i32::MAX))`.
  Both describe `row` (the start). Out-of-view values are the caller's to filter.
- `GridText::oldest()` = `screen.oldest()` at copy time; `rows()` = live rows copied.
  `GridText` is `Send + Sync` (plain owned data; phase two may run on another thread).

## Phase one: `GridText::from_terminal`

Reads `term.screen()` (the active screen; the alternate screen has no history) and
`term.interner().graphemes`. One pass, oldest to newest, over `screen.row(id)`:

```rust
pub struct GridText { oldest: RowId, text: String, rows: Vec<RowText>, columns: Vec<u32> }
struct RowText { text_start: usize, map: ColumnMap }   // + one end sentinel row: text_start = text.len()
enum ColumnMap { Identity, Table { start: usize } }    // Table: columns[start..=start + n]
```

`GridText` and the mapping live in `search.rs` itself, so `literal`, `regex` and the tests
(child modules) read its private fields with no widened visibility.

Per row, cells `0..n` with `n = row.text_end()` (shared helper, files.md: every column of a
`WRAPPED` row, else up to the last non-blank cell; 0 for an unwritten row; D4), plus one when
the cell at `n` is a `WideSpacer` (`text_end` counts spacers as blank, so it stops after a
trailing wide glyph; its spacer must be in the table so a match ends at the pair's end).

| Cell | Emits | Column table entry |
|---|---|---|
| `Scalar(c)` (incl. `'\t'`, `' '`) | `c` | byte offset of the cell's text, relative to `text_start` |
| `Grapheme(id)` | the whole cluster `graphemes.resolve(id)`; `' '` if it resolves to `""` (D3) | same |
| `WideSpacer`, `LeadingWideSpacer` | nothing (D2) | the previous cell's entry (0 at col 0) |

- After the cells: one sentinel entry = the row's text length (bytes).
- After the row: `'\n'` unless the row is `WRAPPED` and not the newest row (D1). So `text` is a
  sequence of logical lines, each ending in `'\n'`; wrapped rows join with nothing between.
- `Identity` when every cell in `0..n` is a non-spacer ASCII scalar: column = byte offset,
  no table entries pushed (the common case costs one byte per cell). Else `Table`.
- Rows have distinct `text_start`s: an unwritten/blank unwrapped row still owns its `'\n'`, and a
  wrapped row holds at least one non-spacer cell (pair invariant; `cols == 1` holds no wide glyph).
- Allocation: reserve `rows` exactly (live row count + 1); `text` and `columns` grow amortised
  (trimmed rows make a full-width reserve wasteful). No per-row allocation.

## Byte offset -> cell (crate-private, used by both matchers)

All offsets are char boundaries. `rel = p - row.text_start`; `cols(i)` = the row's table slice.

| Position | Row | Column |
|---|---|---|
| start `p` | last row with `text_start <= p` | `Identity`: `rel`. `Table`: `v` = largest entry `<= rel`; column = first index whose entry `== v` (rounds into a cluster down to its cell; a glyph wins over its spacer) |
| exclusive end `p` | last row with `text_start < p` | `Identity`: `rel`. `Table`: number of entries `< rel` (rounds up; covers a wide glyph's spacer and a trailing `LeadingWideSpacer`) |
| empty match | both ends use the start rule | |

Row lookups are `partition_point` over `rows` (O(log rows) per match). An end at the seam of a
wrapped row maps to `(row r, cols)`; a start there maps to `(row r + 1, 0)`.

## Limits

| Quantity | Bound | Note |
|---|---|---|
| rows copied | live rows: `scrollback_limit + rows` (<= `SCROLLBACK_MAX + MAX_ROWS`) | embedder-chosen; no cap (a cap would hide the oldest history) |
| text bytes | `<= cells * 64 + rows` (16-scalar cluster cap x 4 bytes) | typical: 1 byte per ASCII cell up to the content end |
| column table | 4 bytes per cell of non-identity rows + 1 per row | `u32`: a row's text is `<= 2048 * 64` bytes |
| `rows` | 16 bytes per row | |
| matches | `MAX_MATCHES` (D6); `len() == MAX_MATCHES` means "possibly more" | 100k x 32 B = 3.2 MB |

- Rustdoc on `from_terminal` states the cost (one pass, memory as above, worked example:
  100 000 rows x 200 columns of ASCII, half full: ~10 MB) and "call under your lock".
- `search_grid_text` allocates the result `Vec` and, for a literal, the folded needle; nothing
  per row. It never panics: every slice index comes from the tables above (use `get`).

## Staleness (rustdoc + guide)

A `GridText` is the grid as it stood. Matches stay valid while their rows are live:
`VtEvent::RowsTrimmed { oldest }` ends rows older than `oldest`; a column resize
(`ResizeOutcome::reflowed`) gives primary rows fresh ids (0005 D1), so re-copy and re-search.
