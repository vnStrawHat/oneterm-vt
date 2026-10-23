# Text: `selection/text.rs`

One private entry point, `fn selection_text(screen: &Screen, graphemes: &GraphemeArena, kind,
range) -> String`, called by `Selection::text` after `to_range`. One `String`, no other allocation.

## Per-row piece

For row `r` and an inclusive column span `c0..=c1` (clamped to `cols - 1`), via `screen.row(r)`:

1. **Content end** `e`: a `WRAPPED` row -> `cols` (every cell belongs to the logical line).
   Otherwise `1 +` the last column whose cell is not **blank**, else 0. Blank = content
   `Scalar(' ')` or `Grapheme(GraphemeId(0))`, as `Cell::is_erasable` (spacers included, any style: a styled background is not text). Scan back from
   `occ` when the last cell is blank (cells at `col >= occ` equal the last cell); an unwritten
   row has `e = 0`.
2. Emit columns `c0..min(c1 + 1, e)`:
   - spacer cells (`WideSpacer`, `LeadingWideSpacer`) emit nothing;
   - `Scalar(c)` -> `push(c)`; `Grapheme(id)` -> `push_str(graphemes.resolve(id))`;
   - **tab run**: after a `Scalar('\t')` cell, skip blank cells until a column where
     `tabs.is_stop(col)` or a non-blank cell; that column is emitted normally. So `a\tb` copies
     as `a`, `'\t'`, `b`, not a tab plus spaces. Ceiling: stops are read now; a `TBC` after the tab
     was printed can shorten or lengthen the skip.

`'\t'` only appears in a cell that held a blank when the tab passed over it (spec 0004 `put_tab`).

## By kind

| Kind | Rows and spans | Between rows | After the last row |
|---|---|---|---|
| `Simple`, `Semantic` | first row `start.col..=cols - 1`, middle rows whole, last row `0..=end.col` (one row: `start.col..=end.col`) | `'\n'` iff the upper row is not `WRAPPED` | nothing |
| `Lines` | same (the range already spans whole lines) | same | exactly one `'\n'` |
| `Block` | every row `start.col..=end.col`; a span starting on a `WideSpacer` starts one column left (the glyph is emitted); the piece is then right-trimmed of `' '` | always `'\n'` | nothing |

- Wrapped rows join with no newline, so a long command line copies as one line.
- A middle row of a non-block range is trimmed only when unwrapped, which is exactly where a
  `'\n'` follows it: trailing blanks before a hard line end never reach the clipboard.
- A block right-trims each piece even on `WRAPPED` rows (the rectangle's right edge is
  arbitrary); `'\t'` survives the trim.
- Pair handling for non-block kinds is already in the range (wide snap, model.md): a range never
  starts on a spacer, and a range that ended on a leading spacer reaches the wrapped glyph.
- A range over blanks only gives `Some("")`; an empty selection gives `None` (`to_range`).

## Worked examples (10 columns)

| Grid | Selection | Text |
|---|---|---|
| `hi` / `there` (hard break) | `Simple` (0,0) L .. (1,9) R | `"hi\nthere"` |
| `0123456789` wraps into `abcde` | `Simple` (0,0) L .. (1,4) R | `"0123456789abcde"` |
| `abcdefghi` + wide `中` (leading spacer at col 9) | `Simple` (0,0) L .. (0,9) R | `"abcdefghi中"` |
| `a中b` | `Simple` (0,2) L .. (0,3) R (start on the spacer) | `"中b"` |
| `a\tb` (tab at col 1, stop 8) | `Simple` (0,0) L .. (0,8) R | `"a\tb"` |
| `abcdefgh` / `kl` / `uvwxyz` | `Block` (0,1) L .. (2,6) R | `"bcdefg\nl\nvwxyz"` |
| `0123456789abcdefghijklmno` | `Lines` at (1,3) | `"0123456789abcdefghijklmno\n"` |

## `Terminal::row_text` (phase 6, not this spec's code)

`Terminal::row_text(&self, id: RowId) -> String` wraps the existing `Screen::row_text` on
`grid.screen_of(id)` with `&interner.graphemes`: the whole row, spacers skipped, graphemes
resolved, `'\t'` cells as tabs and the blanks after them as spaces, **no trimming, no newline**;
an unwritten row gives `cols` spaces. It is a raw row reader, not a copy path: copying goes
through `Selection::text`, whose trim, tab-run and newline rules above are the only ones.
