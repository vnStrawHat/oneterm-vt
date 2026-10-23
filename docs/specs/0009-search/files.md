# Files to touch

Builds on specs 0004-0008 as implemented; phases 6 (`Terminal`) and 7 land first. Only new
dependency: optional `regex` (already in `[workspace.dependencies]`). Sub-milestones, gate green
after each: **8a** model, literal matcher, shared helpers, tests; **8b** feature `regex`, regex
matcher and tests, `docs/user/search.md`.

## `crates/vt/src` (new)

| Path | Content | Milestone |
|---|---|---|
| `search.rs` | module doc (two phases, row ids, cost); `SearchOptions`, `SearchPattern`, `SearchMatch` (+ `grid_line`, `display_row`), `GridText` (+ `from_terminal`, `oldest`, `rows`, private `RowText`, `ColumnMap`, start/end mapping), `MAX_MATCHES`, `search_grid_text`; `mod literal;`, `#[cfg(feature = "regex")] mod regex;`, `#[cfg(test)] mod search_tests;` | 8a |
| `search/literal.rs` | `search`, `fold`, word-boundary check, the scan loop (literal.md) | 8a |
| `search/regex.rs` | `search` over logical lines (regex.md) | 8b |
| `search/search_tests.rs` | tests.md; the regex tests in a `#[cfg(feature = "regex")] mod regex_pattern` | 8a, 8b |

## `crates/vt/src` (edits)

| Path | Change |
|---|---|
| `cell.rs` | `pub(crate) fn is_blank(self) -> bool` on `Cell`: moved verbatim from `selection/text.rs` (`Scalar(' ')`, `Grapheme(GraphemeId(0))` or a spacer) |
| `grid/row.rs` | `pub(crate) fn text_end(&self) -> u16` on `RowRef`: moved verbatim from `selection/text.rs::content_end` (doc: one past the last text column; every column of a `WRAPPED` row) |
| `selection/text.rs` | calls `cell.is_blank()` and `row.text_end()`; its local copies are deleted; no behaviour change (its tests stay green untouched) |
| `lib.rs` | below |

`lib.rs` (crate doc gains: "and [`search`]: literal and regular-expression scrollback search
over a copy of the grid, matches spanning wrapped rows"):

```rust
pub mod search;

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/search.md")]
mod user_guide_search {}
```

Nothing is re-exported at the root: the roadmap names the items under `search::`.

## `crates/vt/Cargo.toml`

```toml
[features]
# Regular-expression scrollback search (`search::SearchPattern::Regex`). Off by default: the
# literal matcher needs nothing, and the default dependency set is a promise.
regex = ["dep:regex"]

[dependencies]
# Optional: the `regex` feature's matcher; default features kept (Unicode classes, perf).
regex = { workspace = true, optional = true }
```

`Cargo.lock` gains `regex`, `regex-automata`, `regex-syntax`, `aho-corasick`. Root `Cargo.toml`:
no change (the `regex = "1.12"` line and its "feature `regex`, phase 8" comment already exist).

## Docs

| Path | Change |
|---|---|
| `docs/user/search.md` | new chapter (tests.md outline); flip row 05 of `docs/user/README.md` to `ready` with a link |
| `docs/user/README.md` Features table | no change (`regex` row already says `search::SearchPattern::Regex`; dep `regex`) |

## Rules

- No `unsafe`; no `unwrap`/`expect`/`panic!`/`unreachable!`/`debug_assert!` on caller input
  outside tests (D8); slice access through `get` where an index is computed.
- `#![warn(missing_docs)]`: every public item documented; rustdoc examples compile in both builds
  (the regex variant's own rustdoc example is `#[cfg(feature = "regex")]`-gated the same way).
- No bench tier (the roadmap names none for phase 8).

## Gate (beyond the quality gate)

```bash
cargo test -p oneterm-vt --no-default-features
cargo test -p oneterm-vt --features regex
cargo clippy -p oneterm-vt --all-targets --features regex -- -D warnings
RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features
```

## Commit

`feat(search): literal and regex scrollback search across wrapped rows` (minor; body names
`Spec: docs/specs/0009-search`).
