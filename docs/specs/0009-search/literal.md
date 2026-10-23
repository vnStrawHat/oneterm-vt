# Literal matcher: `search/literal.rs`

`pub(super) fn search(text: &GridText, needle: &str, options: SearchOptions) -> Vec<SearchMatch>`.
Scans `GridText::text` once, left to right; every hit is mapped with the start/end rules of
model.md. No feature, no new dependency (`memchr` is already a `vt` dependency).

## Rules

| Case | Result |
|---|---|
| `needle` empty | no matches |
| `needle` contains `'\n'` | no matches: a match never crosses a hard line end. Any other needle cannot contain `'\n'`, so scanning the whole text needs no per-line split |
| hit order | top to bottom, left to right; non-overlapping: after an **accepted** hit, scanning resumes at its end |
| rejected hit (`whole_word` fails) | scanning resumes one **char** after the hit's start (D7), not at its end |
| result count | stops at `MAX_MATCHES` (D6) |
| across a wrap | free: wrapped rows are contiguous in `text` (D1); spacers emit nothing (D2) |

## Case folding (`case_sensitive == false`, the default) — D5

```rust
fn fold(c: char) -> char   // 1:1, never changes the char count
```

- ASCII: `c.to_ascii_lowercase()`.
- Non-ASCII: the single char of `c.to_lowercase()` when it yields exactly one char **and** that
  char is non-ASCII; otherwise `c` unchanged. (`ToLowercase` is `ExactSizeIterator`.)
- So `fold` is 1:1 (columns stay exact, the property ASCII-only folding would also give), covers Latin-1,
  Latin Extended, Greek, Cyrillic, Armenian, and **no non-ASCII char folds to ASCII** (U+212A
  KELVIN SIGN stays itself): that is what makes the byte prefilter below exact.
- Not Unicode full or simple case folding: `ß`/`ẞ`, `ς`/`σ`, `ſ`/`s`, `İ` are not unified.
  Rustdoc on `SearchOptions::case_sensitive` says this and points to `(?i)` under `regex`.

## Word characters (`whole_word`) — D5

- Word char: `c.is_alphanumeric() || c == '_'` (Unicode; the regex crate's `\w` is Unicode by
  default, so `\b` and `whole_word` agree on letters).
- Boundary at byte `p` (`is_word_boundary`): true if `p` is the text start
  or end, or the char before `p` or the char at `p` is not a word char. `'\n'` is not a word
  char, so line ends are boundaries. A hit is accepted iff both its start and end are boundaries.
- Neighbours are decoded from `text` (`text[..p].chars().next_back()`, `text[p..].chars().next()`).

## Scanning (allocation-light)

| Mode | Candidate search | Verify |
|---|---|---|
| `case_sensitive` | `memchr::memmem::Finder::new(needle)`; `find(&bytes[pos..])` in a loop (D7 needs restarts, so not `find_iter`). UTF-8 needle in UTF-8 text: every hit is char-aligned | none (exact); whole-word check |
| folded, first folded char `f` ASCII letter | `memchr::memchr2(f, f.to_ascii_uppercase(), &bytes[pos..])` | decode chars from the candidate, `fold(h) == needle_folded[k]` for every `k`, fail fast |
| folded, `f` other ASCII | `memchr::memchr(f, ..)` | same |
| folded, `f` non-ASCII | `text[pos..].char_indices()` with `fold(c) == f` | same |

- `needle_folded: Vec<char>`: the only allocation besides the result.
- A verify that runs into `'\n'` fails at once (the needle holds none).
- Hit length in bytes = bytes consumed in the haystack (may differ from the needle's bytes
  when folding, e.g. `É` vs `é` both 2, Cyrillic both 2; never assumed equal).
- Worst case O(text x needle) for folded search on self-similar text (`aaaa...` with
  `aaa...ab`). Mark it `// ponytail:` in the code: the needle is the user's typing, not program
  output; upgrade path = KMP over folded chars if a profile ever shows it. The case-sensitive
  path is linear (`memmem` is Two-Way + SIMD prefilter).

## Worked examples (one row unless stated)

| Text | Needle, options | Matches `(start_col, end_col)` |
|---|---|---|
| `Hello World` | `world` | `(6, 11)` |
| `Hello World` | `hello`, case-sensitive | none |
| `foo bar foo baz foo` | `foo` | `(0,3) (8,11) (16,19)` |
| `foo foobar foo` | `foo`, whole word | `(0,3) (11,14)` |
| `foo_bar foo` | `foo`, whole word | `(8,11)` |
| `aa-a-a` | `a-a`, whole word | `(3,6)` (D7; resuming at the hit's end finds none) |
| `日本語` | `本語` | `(2,6)` (D2; a char-per-cell copy finds none) |
| `ПРИВЕТ мир` | `привет` | `(0,6)` (D5; ASCII folding finds none) |
| 10 cols, `0123456789` wraps into `abc` | `89ab` | row 0 col 8 .. row 1 col 2 (D1) |
| 10 cols, `abcdefghi` + `中` (leading spacer col 9, `中` on row 1) | `i中` | row 0 col 8 .. row 1 col 2 |
| `a` + `\r\n` + `b` (hard break) | `ab` | none |
