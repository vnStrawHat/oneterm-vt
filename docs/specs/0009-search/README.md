# 0009: Search (phase 8)

**Goal:** `search::{GridText, SearchMatch, SearchOptions, SearchPattern, search_grid_text}`: copy
the active screen's text under the lock, match without it; literal search with case folding and
whole words; matches that span wrapped rows; feature `regex` adds `SearchPattern::Regex`.

**Non-goals:** incremental or streaming search, match highlighting in snapshots, a search
cursor/"next" state, tab-run collapsing (copy does it, search does not), bench tier.

| File | Covers |
|---|---|
| [model.md](model.md) | public items, `GridText` layout, wrap joining, spacers, clusters, offset -> cell mapping, limits |
| [literal.md](literal.md) | literal rules, folding scope, word chars, memchr/memmem scan, worked examples |
| [regex.md](regex.md) | feature and dependency, `Regex` variant, per-logical-line matching, `docs/user/search.md` gating |
| [tests.md](tests.md) | unit tests, golden table run in every build, hostile rows, chapter outline |
| [files.md](files.md) | files, shared helpers moved from `selection`, `lib.rs`, Cargo, gate, commit |

**Public API:** `search::{GridText::{from_terminal, oldest, rows}, SearchMatch::{row, start_col,
end_col, grid_line, display_row}, SearchOptions { case_sensitive, whole_word }, SearchPattern::
{Literal, Regex} (non_exhaustive), search_grid_text}` + `SearchMatch::end_row` (D1) + `MAX_MATCHES` (D6).

## Acceptance criteria

- [ ] Quality gate; `cargo test -p oneterm-vt` with `--no-default-features` and `--features regex`; `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features`
- [ ] Same literal results with and without `--features regex` (golden table, every CI config)
- [ ] Matches across wrapped rows, literal and regex (tests.md)
- [ ] A 1M-cell logical line searches without panic; results capped at `MAX_MATCHES`
- [ ] `docs/user/search.md` exists; its Rust blocks run under `cargo test --doc` in both builds
- [ ] Default build has no `regex` in `cargo tree -e normal`; no `unsafe`; no panics on caller input
- [ ] Commit: `feat(search): literal and regex scrollback search across wrapped rows` (minor)

## Decisions (settled)

1. Two phases; `GridText` owns plain data (`Send + Sync`); it searches the active screen only.
2. Text = UTF-8 logical lines, `'\n'`-terminated, a column table only for non-ASCII rows (model.md).
3. A `'\t'` cell searches as `'\t'`, blanks as `' '`; an empty needle or one with `'\n'` finds nothing.
4. `whole_word` uses the rule "either side non-word" (literal.md).
5. Regex: options ignored, caller compiles, default features kept; one chapter file, gated by two `cfg`'d `main`s.
6. No cap on copy size (embedder-chosen history), cost documented; `display_row` keeps `usize`, saturating.
7. `Cell::is_blank` / `RowRef::text_end` move out of `selection/text.rs` and are shared (files.md).

## Design choices with alternatives (reviewer rules A = as proposed, B = the simpler alternative)

| # | Proposed (A) | Simpler alternative (B) | Evidence |
|---|---|---|---|
| D1 | wrapped rows join into one logical line; `SearchMatch` gains `pub end_row: RowId` | match each row alone; no match spans rows | roadmap phase 8 acceptance requires matches across wrapped rows; per-row matching misses every word a wrap splits |
| D2 | spacer cells emit nothing; a column table maps chars back | spacer = `'\0'` in a char-per-cell copy, compared cell by cell | `日本` (4 cells) would never match the 2-char needle |
| D3 | a cluster cell emits its whole cluster | first scalar only (`text_char`) | `👍🏽`, flags `🇯🇵`, decomposed `é` could never match. Cost of A: up to 64 bytes per cell on hostile cluster rows (model.md limits) |
| D4 | an unwrapped row ends at its content end (selection's trim rule, 0006 D8); `$` = content end | rows padded to grid width with spaces | `error$` would never match a short line; padding also costs 4 bytes per empty cell |
| D5 | 1:1 Unicode lowercase folding (never non-ASCII to ASCII); word char = `is_alphanumeric() \|\| '_'` | ASCII folding and ASCII word chars | the default (case-insensitive) would be silently case-sensitive for Cyrillic/Greek/accented text; regex `\w`/`(?i)` are Unicode, so the two patterns would disagree |
| D6 | `pub const MAX_MATCHES: usize = 100_000`; results stop there | unbounded `Vec` | grid output of one letter + a one-letter needle gives one 24-byte match per cell (6x the copy); roadmap rule "never grows memory without a bound" |
| D7 | a hit rejected by `whole_word` resumes one char later | resume after the rejected hit | `aa-a-a`, needle `a-a`, whole word: the valid hit at col 3 would be lost |
| D8 | no assertion: options are ignored for a regex in every build | `debug_assert!` on non-default options | code style "no `panic!()` in production code"; a UI keeping its "match case" toggle while in regex mode would panic in debug; precedent 0008 D1 |

Rows are independent (B for one keeps the rest valid); B for D1 fails the roadmap acceptance.

## Open items

None. Checked against the roadmap, the regex crate's documented semantics, Cargo feature and
doctest gating, and our grid/selection/terminal specs; every remaining question became a
Decision or a design choice above.
