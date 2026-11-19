# Search: regex and your own matcher

[`search.md`](search.md) covers the copy and the literal matcher. A pattern is run either by the
engine with the `regex` crate, or by a matcher you already ship.

## Who does what

| The engine | You |
|---|---|
| copies the text and knows which cell each byte of it came from: `GridText::span` | choose the matcher; its syntax, compile cost and size limits are yours |
| feature `regex`: runs your compiled `regex::Regex` over each logical line, at most `MAX_MATCHES` | compile the pattern |
| your own matcher: turns each byte range you found into a `SearchMatch` | walk the lines, run the matcher, cap the result |

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

## Your own matcher: `GridText::span`

No feature needed. Run any matcher (another regex crate, a fuzzy finder) over
`GridText::as_str()`, one logical line at a time, and `span(start, end)` turns the byte range of
a hit into the `SearchMatch` the engine's matchers give for it.

```rust
use std::ops::Range;
use std::time::Instant;
use oneterm_vt::search::{GridText, MAX_MATCHES, SearchMatch};
use oneterm_vt::{Config, EventBatch, Size, Terminal};

/// Every hit of `find_in_line` (your matcher: byte ranges within the line it is given).
fn search_with(text: &GridText, find_in_line: impl Fn(&str) -> Vec<Range<usize>>) -> Vec<SearchMatch> {
    let mut found = Vec::new();
    let mut line_start = 0;
    for line in text.as_str().split_inclusive('\n') {
        // The matcher does not see the line end, so `$` and `\s` cannot match it.
        let content = line.strip_suffix('\n').unwrap_or(line);
        for hit in find_in_line(content) {
            if found.len() == MAX_MATCHES {
                return found; // the cap is yours here
            }
            found.extend(text.span(line_start + hit.start, line_start + hit.end));
        }
        line_start += line.len();
    }
    found
}

let mut term = Terminal::new(Size { rows: 3, cols: 10 }, Config::default());
term.feed(b"error 404 not found", &mut EventBatch::new(), Instant::now());
let text = GridText::from_terminal(&term);

// A stand-in for your matcher.
let found = search_with(&text, |line| {
    line.match_indices("4 not").map(|(at, hit)| at..at + hit.len()).collect()
});
assert_eq!(found.len(), 1);
// The line wraps after ten columns: the match starts on one row and ends on the next.
assert_eq!((found[0].start_col, found[0].end_col), (8, 3));
assert_eq!(found[0].end_row, found[0].row + 1);
```

Rules of `span`:

- The offsets are bytes of the whole `as_str()`: add the line's start to what your matcher
  returns for a line.
- An offset inside a cell's text (a cluster, a char of several bytes) takes the whole cell: the
  start rounds down to it, the end rounds up past it. A matcher over bytes is fine.
- `start == end` is an empty match at that cell; at a line's `'\n'` it is the line's content end.
- `None` when the range is not within one line: `start > end`, an end past the text, a start at
  the text's end, a `'\n'` inside the range. For a hit over several lines, split it at the line
  ends and ask for each part.
- `MAX_MATCHES` is not applied. Each call is two binary searches over the rows.
- The matches are as live as any other: see "A live grid" in [`search.md`](search.md).
