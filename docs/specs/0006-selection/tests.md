# Tests

Fixture: the grid's `grid_fixture::Fixture` (`print`, `newline`, `goto`, `check`, `resize` from
phase 4), made reachable with `#[cfg(test)] pub(crate) mod grid_fixture;`. Local helpers in
`selection_tests.rs`: `at(index: i32, col) -> Pos` (screen index; negative = history),
`select(kind, (Pos, Side), (Pos, Side)) -> Selection`, `span(kind, from, to)` (= `Left` .. `Right`),
`range(&sel)`, `text(&sel)` (default escape set). Every test ends with `f.check()` (tier-3 walk:
selection anchors stay inside their lane). Notation `5x10` = rows x cols, scrollback 100.

## `selection/selection_tests.rs` (`mod selection_tests;` in `selection.rs`)

| Group | Test: setup -> expected |
|---|---|
| simple | `side_rules`: 5x10 `abcdefghij`, cells 2 and 5: `L..R` `cdef`, `R..L` `de`, `L..L` `cde`, `R..R` `def` |
| | `empty_when_no_whole_cell`: same pos same side; `(3,R)..(4,L)` and reversed -> `None`; `(3,L)..(3,R)` and `(3,R)..(3,L)` -> `d` |
| | `row_edge_half_cells_keep_last_cell`: 5x10 `abcdefghij` CRLF `klmnopqrst`, `(0,9,R)..(1,0,L)` -> range `(0,9)..=(0,9)`, text `j` (Decision 3) |
| | `reversed_ends_same_range_every_kind`: swap the two (pos, side) pairs -> equal `to_range` for all 4 kinds |
| block | `block_columns_from_sides` (corners top-right/bottom-left normalise; rows keep both ends) |
| | `block_one_column_and_empty` (`(c,R)..(c+1,L)` -> `None`; `(c,L)..(c,R)` on two rows -> column `c`; `(c,R)..(c,L)` on two rows -> `None`, columns would cross) |
| | `block_text_rectangle` (`cde\nmno\nwxy`); `block_trims_each_row` (`bcdefg\nl\nvwxyz`) |
| | `block_wide_edges`: `中文abc` / `xy中zz`, (0,1)..(1,3) -> `中文\ny中`; `contains_cell` paints both halves of a cut pair |
| semantic | `word_uses_escape_chars`: `foo bar-baz qux` at col 5 -> `bar-baz`; with `" -"` -> `bar` |
| | `word_stops_at_hard_line_end` (`abc` CRLF `def`, click (1,1) -> `def`, start col 0) |
| | `word_crosses_wrap` (`abcdefghijklm` at 10 cols, click (1,0) -> all 13) |
| | `word_over_wide_chars` (`中文 abc`, click (0,0) or (0,1) -> `中文`) |
| | `click_on_delimiter_takes_both_words` (`abc def`, click the space -> `abc def`) |
| | `grapheme_cell_is_never_escape` (`e\u{301}` between letters stays in the word) |
| | `bracket_match_both_directions` (`foo(bar(baz))`, (0,3) and (0,12) -> same range, text `(bar(baz))`) |
| | `bracket_without_match_falls_back_to_word`; `bracket_across_rows` |
| | `semantic_never_empty` (`(1,R)..(1,L)` same cell -> `Some`) |
| | `expansion_budget_caps_scan`: `word_start`/`word_end`/`bracket_match`/line walk called with `budget = 4` on a 3-row wrapped word -> each stops 4 cells (rows) away; `to_range` passes `EXPAND_BUDGET` (65,536) |
| lines | `lines_follow_wraps` (`0123456789abcdefghijklmno`, click (1,3) -> (0,0)..=(2,9), text ends `"\n"`) |
| | `lines_never_empty`; `lines_two_logical_lines` (drag across a hard break -> `"l1\nl2\n"`) |
| text | `wrapped_rows_join`; `hard_break_newline_and_trim`; `styled_blanks_trimmed` (bg-styled spaces after text) |
| | `spacers_skipped_graphemes_resolved` (`a中b` + `e\u{301}` -> `a中be\u{301}`); `start_on_spacer_takes_glyph` |
| | `leading_spacer_end_takes_wrapped_glyph` (`abcdefghi中`, (0,0)..(0,9) -> `abcdefghi中`) |
| | `tab_run_copies_as_tab` (`a\tb` -> `a\tb`); `blanks_only_is_empty_string` (`Some("")`); `empty_selection_text_none` |
| api | `select_all_covers_history` (3x10, 8 lines: range (oldest,0)..(newest,9); text starts `line0`) |
| | `release_frees_both_anchors` (`Anchors::live()` back to the count before `new`) |
| | `positions_clamp_into_lane` (row past `newest`, col past `cols` -> clamped; `check` passes) |
| | `ends_in_two_lanes_resolve_none`; `selection_on_inactive_screen_reads_its_lane` |
| | `contains_rules` (simple multi-row middle cols; block excludes outside cols); `contains_cell_wide_pair` |
| follow | `survives_scroll_into_history` (3x10, 5 more lines: same text, start = `at(-5, 0)`) |
| | `trimmed_end_resolves_none` (scrollback 2); `viewport_scroll_changes_nothing` |
| | `region_scroll_moves_selection`; `delete_lines_kills_end_on_deleted_row` |
| | `survives_rows_only_resize`; `survives_column_reflow` (26 letters at 10 cols, (0,2)..(1,4) `cdefghijklmno`; cols 6 then 10: same text both times) |
| | `alt_clip_clamps_end_col` (alt active, shrink cols: range still `Some`, `end.col < cols`) |
| invalidation | `invalidation_table`: 5x10 rows `aaa`..`eee`, selection rows 1..=2, per (variant, cursor row, expected): EL (3,F) (2,T); ED0 (3,F) (1,T) (2,T); ED1 (0,F) (1,T) (3,T); ED2 (any,T); ED3 (F); SwapAlt T; Reset T |
| | `history_selection_rules`: selection wholly in history: ED3 T, ED2 T; ED0/1 and EL F |
| | `inactive_screen_uses_its_own_cursor` (selection on primary rows 1..=2, primary cursor on row 2, alt active with its cursor on row 4: EL T, ED0 T; SwapAlt T) |
| | `dead_end_invalidates_everything` (trim the start: every variant T, `to_range` `None`) |

## Proptests (`selection/selection_props.rs`, `mod selection_props;` in `selection.rs`)

Grid 6x8, scrollback 64; content = random strings over `a..=z` printed with random CRLFs (lines
wrap); ends anywhere live, random sides.

1. **`simple_text_is_grid_slice`**: dense ASCII only: lines are a multiple of `cols` long,
   CRLF between them, at least `rows` rows written; ends are drawn from written rows only, so no
   cell in play is blank. Oracle, independent of `text.rs`: walk the cells of `to_range` in order,
   push each scalar, `'\n'` after an unwrapped row unless it is the last. Equal to `text()`.
   Also: `to_range` `None` only by the `Simple` empty rule; `start <= end`; inside live rows.
2. **`survives_scroll`**: any selection with a range, then `n in 1..20` LFs at the bottom (no
   trim: `prop_assume!` history fits) -> `to_range` equal, `text` equal.
3. **`well_formed_after_ops`**: any kind, then a random op (region scroll up/down, IL, DL, LF,
   resize within 4..10 x 4..12): `to_range` is `None` or ordered, live, `col < cols` (block:
   `start.col <= end.col`); `text` never panics; `check()` passes.

## User chapter `docs/user/selection.md` (row 17; coder writes, <= 120 lines, all doctests)

1. Example: `TerminalGrid::new`, write `hello world` with `row_mut` + `set`,
   `Selection::new(.., Semantic, ..)` at the `w`, `text(..)` == `"world"`, `release`.
2. Kinds and `Side`: table; the side rule in two sentences; wide glyphs are atomic.
3. Text rules: wrapped rows join, trailing blanks trimmed, tabs, `Lines` ends with `\n`, block.
4. Following content: survives scroll and reflow; `to_range` `None` once content is gone.
5. Embedder checklist: ask `invalidated_by` before erases (table of VT op -> variant);
   always `release`; pass the word delimiters you want as `escape_chars`.
