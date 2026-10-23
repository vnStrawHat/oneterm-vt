# Tests

Unit tests in `search/search_tests.rs` (`#[cfg(test)] mod search_tests;` in `search.rs`), driven
through a real `Terminal` and `feed` (no grid internals). Fixture: `terminal(cols, rows, bytes)`
(default `Config`, one `feed`), `find(&Terminal, needle, SearchOptions) -> Vec<SearchMatch>`
(copy + literal search), `hits(&[SearchMatch], top) -> Vec<(i32, usize, i32, usize)>`
(`grid_line` of `row` and `end_row`, cols). Options via `SearchOptions { .. }` (crate-internal).
One behaviour per test.

## Model and mapping

| Test | Steps -> assert |
|---|---|
| `copy_is_keyed_by_row_id` | 2 x 6, `alpha\r\nbeta\r\nalpha` -> `oldest() == screen.oldest()`, `rows() == 3`; drop the terminal, search `alpha` -> grid lines `-1` and `1` |
| `display_row_adds_the_offset` | same -> `m[0].display_row(top, 1) == 0`; `display_row(top, usize::MAX)` saturates, no panic |
| `grid_line_clamps` | `SearchMatch { row: RowId(u64::MAX), .. }.grid_line(RowId(0)) == i32::MAX` |
| `alt_screen_is_searched_when_active` | `ESC[?1049h` + text -> match ids in the alt lane (`>= RowId::ALT_ORIGIN`) |
| `blank_rows_and_unwritten_rows_copy_empty` | 3 x 5, nothing fed -> `rows() == 3`, literal `" "` finds nothing (D4) |
| `grid_text_is_send_and_sync` | `fn assert_send_sync<T: Send + Sync>()` on `GridText` |

## Literal (one test per literal.md row and worked example)

`empty_needle_finds_nothing`, `newline_needle_finds_nothing`, `default_is_case_insensitive`,
`case_sensitive_respects_case`, `matches_do_not_overlap`, `matches_run_top_to_bottom`,
`whole_word_excludes_substrings`, `whole_word_counts_underscore`, `rejected_hit_does_not_hide_the_next`
(D7, `aa-a-a`), `wide_needle_spans_spacers` (D2, `日本語`/`本語` -> `(2, 6)`; `日` -> `(0, 2)`),
`cluster_is_searched_whole` (D3: `👍🏽` printed, needle `👍🏽` -> one hit `(0, 2)`; needle `👍` ->
one hit too, mapped to the same cell), `cyrillic_folds` (D5), `kelvin_sign_does_not_fold_to_ascii`
(`K` U+212A vs needle `k` -> none), `unicode_letters_are_word_chars` (`fooé foo`, whole word ->
`(5, 8)` only), `trailing_blanks_are_not_text` (`foo   ` + `\r\n`, needle `foo ` -> none; D4),
`tab_cell_is_a_tab` (`a\tb` -> needle `a\t` hits `(0, 2)`).

## Across wrapped rows (roadmap acceptance)

| Test | Steps -> assert |
|---|---|
| `match_spans_a_wrap` | 3 x 10, `0123456789abc` -> `89ab` = `(0, 8, 1, 2)` (start row, col, end row, col) |
| `match_ends_on_the_last_column` | same, `789` -> `(0, 7, 0, 10)`, `end_row == row` |
| `match_spans_three_rows` | 4 x 4, 10 letters -> needle of 8 from col 1 -> `(0, 1, 2, 1)` |
| `leading_spacer_is_skipped` | 3 x 10, `abcdefghi中` -> `i中` = `(0, 8, 1, 2)` |
| `hard_break_is_not_joined` | `a\r\nb` -> `ab` none; `a\nb` (LF only, `b` at col 1 of the next row) -> `ab` none |
| `wrap_join_survives_scrollback` | 2 x 5, `0123456789xyz` -> `34567` spans a history row and a screen row |

## Limits and hostile rows

| Test | Steps -> assert |
|---|---|
| `one_hostile_logical_line` | 500 x 2000, feed 1 000 000 `a` (one logical line over 500 rows); needle `aaab` -> none; needle `a` with `whole_word` -> none |
| `match_count_is_capped` | same grid, needle `a` -> `len() == MAX_MATCHES`, last hit ordered after the first |
| `cluster_heavy_row_maps_columns` | 1 x 20, twenty cells of `e` + 15 x U+0301 (cut at 16 scalars) -> that cluster as needle hits every cell, `(i, i + 1)` |
| `ascii_rows_store_no_column_table` | crate-internal: ASCII-only grid -> `columns.is_empty()` (identity rows) |

## Regex (`#[cfg(feature = "regex")] mod regex_pattern`)

| Test | Steps -> assert |
|---|---|
| `escaped_needle_matches_the_literal` | for every golden row below without `whole_word`: `Regex::new(&escape(n))` (+ `(?i)` when folded) == literal result |
| `caret_anchors_at_line_start` | `foo foo`, `^foo` -> one hit `(0, 3)`; a wrapped line's second row is not a line start |
| `dollar_anchors_at_content_end` | 1 x 5, `ab` -> `b$` hits `(1, 2)` (D4) |
| `regex_spans_a_wrap` | `0123456789abc` in 10 cols, `9a` -> `(0, 9, 1, 1)` |
| `empty_matching_pattern_terminates` | 1 x 2, `ab`, `x*` -> starts `[0, 1, 2]`, all empty |
| `options_are_ignored_for_regex` | non-default options -> same result as default, no panic in debug (D8) |
| `regex_match_count_is_capped` | the hostile line, `a` -> `MAX_MATCHES` |

## Golden literal table (feature-independent, roadmap acceptance)

`literal_golden_table`: not gated; a `const` table of `(bytes, cols, needle, case_sensitive,
whole_word, expected hits)` covering every literal.md worked example. It runs in every CI
configuration, so a feature that changed literal results fails it. The regex agreement test
reuses the same table.

## `docs/user/search.md` (<= 120 lines; every Rust block a doctest)

1. Intro: two phases, why the copy (lock), what a match names; cost of `from_terminal`.
2. Example: `Hello World`, copy, search `world`, assert `(6, 11)`.
3. What a match means: `row`/`start_col` .. `end_row`/`end_col`; painting a multi-row match;
   `grid_line`, `display_row` with `screen().scroll_offset() as usize`; filter out-of-view.
4. Wrapped lines: one logical line, matches span rows; hard line ends never; trailing blanks.
5. Literal options: `case_sensitive` (folding scope), `whole_word` (word chars); build
   `SearchOptions` by mutating `default()` (`#[non_exhaustive]`). Example `cargo` vs `cargofmt`.
6. Limits: `MAX_MATCHES` and how to tell "more"; memory of a copy.
7. A live grid: matches valid while rows are live; `RowsTrimmed`, column resize -> re-search.
8. Regex (feature `regex`): enable it, the two-`main` example (regex.md), anchors, options
   ignored, compilation is yours, empty matches; `match` on `SearchPattern` needs a wildcard arm.
