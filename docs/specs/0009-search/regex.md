# Regex matcher: `search/regex.rs` (feature `regex`)

## Feature and dependency

| Item | Value |
|---|---|
| `crates/vt/Cargo.toml` feature | `regex = ["dep:regex"]` (replaces today's empty `regex = []`); off by default |
| dependency | `regex = { workspace = true, optional = true }`; workspace already pins `regex = "1.12"` |
| default features | kept on (`std`, `perf`, `unicode`): `unicode` makes `\w`, `\b`, `(?i)` Unicode-aware (D5 agrees with it), `perf` is the literal/DFA speed. Adds `regex-automata`, `regex-syntax`, `aho-corasick` (`memchr` is already ours) |
| default build | unchanged: `cargo tree -p oneterm-vt -e normal --no-default-features` must not list `regex` (phase-11 CI check) |
| profiles | no `opt-level` override for the regex crates (no bench tier uses them) |

Inside `search/`, refer to the crate as `::regex::Regex` (the module is also named `regex`).

## `SearchPattern::Regex(&'a ::regex::Regex)`

`#[cfg(feature = "regex")]` variant. Rustdoc (no `doc(cfg)`: the crate does not use `docsrs`)
starts with "Available with the `regex` feature." and states:

- compilation, flags and `RegexBuilder::size_limit` are the caller's; the crate takes a finished
  `Regex`, so a hostile pattern never becomes this crate's problem;
- the pattern runs over one **logical line** at a time (wrapped rows joined, D1); `^`/`$` anchor
  to the line's first char and its content end (trailing blanks of the last row trimmed, D4); a
  wide glyph is one char; a cluster is all its scalars (D3); `\n` never matches;
- `SearchOptions` is ignored for this variant in every build (D8): write `(?i)`, `\b`;
- an empty-matching pattern yields one empty match per char boundary of each line, the end
  included; on a blank row that is one match at column 0.

`search_grid_text`: `SearchPattern::Regex(re) => regex::search(text, re)`. The `match` arm is
`#[cfg(feature = "regex")]`; without the feature the enum has one variant and the `match` is
exhaustive inside the crate (`#[non_exhaustive]` only binds other crates).

## `pub(super) fn search(text: &GridText, re: &::regex::Regex) -> Vec<SearchMatch>`

```text
start = 0
for nl in memchr::memchr_iter(b'\n', text.bytes):        // one '\n' ends every logical line
    line = &text[start..nl]
    for m in re.find_iter(line):                           // non-overlapping; steps past empty matches
        push map(start + m.start(), start + m.end())       // model.md rules; empty: start rule twice
        if len == MAX_MATCHES: return                      // D6
    start = nl + 1
```

- No per-row `String` rebuild and no offset table rebuild: `GridText` already holds UTF-8 text
  and the column tables.
- Runtime: the regex crate guarantees O(pattern x haystack) search; a logical line can be the
  whole history (output with no newline), still linear. Lazy DFA cache growth is bounded by the
  regex crate's own limits; nothing to add here.
- No allocation besides the result `Vec` (the regex crate's per-thread cache aside).

## Literal vs regex agreement (roadmap acceptance)

- The literal matcher is identical code in both builds; the feature only adds a variant and a
  module. The golden literal test (tests.md) runs in every CI configuration
  (`--no-default-features`, default, `--all-features`), so "same literal results with and without
  `--features regex`" is checked by construction plus that test.
- Under the feature, `Regex::new(&regex::escape(needle))` must return exactly the literal
  matcher's `Vec` (case-sensitive), and `(?i)` + escaped needle must equal the folded literal on
  ASCII, Latin-1 and Cyrillic text. Not on `ſ`, `ς`, `K` (U+212A), `ß`: `(?i)` is Unicode
  simple case folding, the literal fold is 1:1 lowercase (literal.md); the rustdoc says so.
- `whole_word` has no exact regex spelling (our boundary is "either side non-word",
  `\b` is "exactly one side word"); no agreement test for it.

## `docs/user/search.md`: one file, regex section compiled both ways

The chapter is included under `#[cfg(doctest)]` like the others (files.md). The regex block
spells the gate in visible code, so it compiles (and does nothing) without the feature and runs
with it; a reader on GitHub sees exactly what to write:

```rust
#[cfg(feature = "regex")]
fn main() {
    use oneterm_vt::search::{GridText, SearchOptions, SearchPattern, search_grid_text};
    // ... build a Terminal, feed "error 404 not found", copy, search r"error\s+\d+", assert 1 hit
}
#[cfg(not(feature = "regex"))]
fn main() {}
```

- Rustdoc passes the crate's `--cfg feature=...` to doctests and links enabled optional deps,
  so `regex::Regex` resolves exactly when the block body is compiled.
- The `use` lines sit inside `main` so the empty build has no unused import.
- Prose before the block says the section needs `features = ["regex"]` and what it adds (the
  alternative of swapping in a second file via `cfg_attr(doc = include_str!)` keeps two
  texts in sync by hand; one file with a visible gate does not).
