# `GridText::span`: a matcher of the embedder's own (added after the phase)

**Why:** `SearchPattern::Regex` takes a `&regex::Regex`, so a pattern search needs the `regex`
crate in the embedder's tree. An embedder that ships another matcher had no way from a byte range
of `as_str()` back to cells: the mapping (model.md, "Byte offset -> cell") was crate-private.

**Split:** the matcher is the embedder's (syntax, compile cost, limits, the loop over lines, the
cap). The mapping is the engine's: only it knows the column tables.

**Rejected:** a `SearchPattern::Matcher(&dyn Fn(..))` variant. It puts a callback into a
`Copy + Debug` enum and makes the engine define how a foreign matcher is called; `span` needs
neither and serves matchers that are not line-callbacks at all (fuzzy finders, byte regexes).

## API

```rust
impl GridText { pub fn span(&self, start: usize, end: usize) -> Option<SearchMatch>; }
```

The private mapping keeps its code under the name `line_span` (both engine matchers call it with
ranges they know to lie in one line); `span` is that plus the checks an outside caller needs.

| Input | Result |
|---|---|
| `start..end` inside one logical line, `start < end` | the match, by the start and end rules of model.md |
| `start == end`, `start < text.len()` | an empty match by the start rule (on a line's `'\n'`: `(last row of the line, content end)`) |
| an offset that is not a char boundary, or inside a cluster | as any offset inside a cell's text: start rounds down to the cell, end rounds up past it |
| `start > end`, `end > text.len()`, `start == text.len()` | `None` |
| a `'\n'` in `text[start..end]` | `None` |

- `start == text.len()` is refused because it would land on the end sentinel row, which is not a
  row of the grid (the text always ends in `'\n'`, so no line starts there).
- A range across a `'\n'` is refused, not clamped: the end rule would give `content end + 1` on
  the line's last row, a column that may not exist, and a match never crosses a line end in
  either engine matcher (D1). The embedder splits such a hit at the line ends.
- The range is read as bytes (`as_bytes().get(start..end)`), not as `str`, so that offsets of a
  byte matcher are rounded instead of refused.
- Cost: `memchr` over the range plus two `partition_point`s over the rows. `MAX_MATCHES` is not
  applied (there is no result list here).
- Never panics: every index goes through `get`.

## Tests (`search/search_tests.rs`)

| Test | Checks |
|---|---|
| `span_gives_an_own_matcher_the_matches_of_the_literal_one` | `str::match_indices` per line + `span` == `search_grid_text` (case-sensitive literal) over wraps, wide glyphs, a cluster, a blank row, a full row |
| `span_maps_a_wrap_seam_as_the_matchers_do` | an end at the seam stays on the first row, a start there is the next row's column 0 |
| `span_takes_the_whole_cell_of_an_offset_inside_its_text` | mid-char and mid-cluster offsets |
| `span_of_an_empty_range_is_one_cell_edge` | empty ranges: line start, on `'\n'` of a full row (`cols`), a blank row |
| `span_refuses_a_range_that_is_not_in_one_line` | every `None` row of the table above; a range up to the line end is accepted |

## Docs

`docs/user/search-matchers.md` (chapter 23): "Who does what", the regex section moved from
`search.md`, and an example loop over `split_inclusive('\n')` with a stand-in matcher (a doctest
in every build). `api-map.md` names `span` on the `GridText` row.
