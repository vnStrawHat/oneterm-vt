# Expansion: `selection/expand.rs`

Private functions over `&Screen` (the selection's lane) returning `Pos`. They read cells through
`Screen::row(id).cell(col)` and `RowRef::wrapped`; no interner, no allocation.

## Escape characters

- `escape_chars: &str` is passed in; phase 6 stores it in `Config::semantic_escape_chars`.
- Default (crate const in `selection.rs`, `Config::default` uses it from phase 6):

```rust
pub(crate) const DEFAULT_SEMANTIC_ESCAPE_CHARS: &str = ",│`|:\"' ()[]{}<>\t";
```

  Until phase 6 reads it, it carries the grid's self-removing pattern:
  `#[cfg_attr(not(test), expect(dead_code, reason = "Config::default reads it from phase 6"))]`.
- A cell **is an escape** iff it is not a spacer, its content is `CellContent::Scalar(c)` and
  `escape_chars.contains(c)`. A grapheme cell never is (the set holds lone scalars, and resolving
  would need the interner). An empty set: words run to the line edges.

## Stream order helpers

`next(p)`: `col + 1 < cols` -> same row; else `(row + 1, 0)` if `row < newest`; else none.
`prev(p)`: mirror, none before `(oldest, 0)`. The `Simple` rule (model.md) steps cells with the
same arithmetic; its steps never pass the other end, so they stay inside the live rows.

## Scan budget

`const EXPAND_BUDGET: u32 = 65_536;` (private). Every scan function takes `budget: u32` (tests
pass a small one); `to_range` passes this. Each word or bracket scan visits at most this many
cells per direction; each line walk at most this many rows per direction. On exhaustion the scan
stops where it is (a word or line longer than that is cut; a bracket match is not found). Reason:
`to_range` runs per snapshot; without a cap one hostile wrapped line makes every frame
O(history).

## Word (`Semantic`, ends `lo <= hi`, sides ignored)

`start = word_start(lo)`, `end = word_end(hi)`. The endpoint's own cell is not tested, so a click
on a delimiter takes the words on both sides of it.

| Search | Rule |
|---|---|
| `word_start(p)` | loop: `q = prev(p)`; stop if none, if `q` is on the row above and that row is not `WRAPPED`, if `q` is an escape, or on budget; else `p = q`. Result `p` |
| `word_end(p)` | loop: stop if `p` is the last column of a row that is not `WRAPPED`; `q = next(p)`; stop if none, if `q` is an escape, or on budget; else `p = q`. Result `p` |

Spacers are never escapes, so a scan walks through wide pairs; the wide snap (model.md) then
makes the ends whole glyphs. A word never crosses a hard line end, and always crosses a wrap.

## Bracket match (`Semantic`, only when `lo == hi`)

- Pairs `('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')`; the cell must be a non-spacer
  `Scalar` in one of them. An opener searches forward, a closer backward.
- Walk `next`/`prev` across rows regardless of `WRAPPED` (code spans lines), budgeted. Count
  depth on the same pair only: the same bracket `+1`, its partner at depth 0 -> found, else `-1`.
- Found -> range `min(p, match) ..= max(p, match)` (no word expansion). Not found -> word rule.

## Lines (`Lines`, sides ignored)

| End | Rule |
|---|---|
| start | `row = lo.row`; while `row > oldest` and `row(row - 1).wrapped()` (budgeted): `row -= 1`; col 0 |
| end | `row = hi.row`; while `row < newest` and `row(row).wrapped()` (budgeted): `row += 1`; col `cols - 1` |

A logical line ending at `newest` with `WRAPPED` set (the cursor row mid-wrap) simply ends there.

## Block (no expansion)

Rows are the two anchor rows ordered; columns come from the `Block` rule (model.md). A
block may cross wrapped rows: each row is its own line of the rectangle (text.md).

## `Side` per kind

| Kind | Effect of `Side` |
|---|---|
| `Simple` | `Left` includes the cell at a start and excludes it at an end; `Right` the reverse (model.md) |
| `Block` | same on columns only; rows always include both anchor rows |
| `Semantic`, `Lines` | ignored: the whole word or line is taken whichever half was hit |

Ordering never depends on the drag direction: swapping the two (pos, side) pairs gives the same
range for every kind (test).
