# Tests

Fixture (`grid/grid_fixture.rs`) gains: `resize(rows, cols, policy) -> ResizeOutcome` (ends with
`check()`), `lines() -> Vec<String>` (logical lines oldest first: rows joined on `WRAPPED`, spacers
skipped, trailing spaces trimmed), `cursor_at() -> (line, offset, pending)`, `mark(index, col)`.
Notation: `3x10` = rows x cols; scrollback 10 unless stated.

## `grid/resize_tests.rs` (`#[path]` child of `terminal_grid.rs`, through the fixture)

| Group | Test: setup -> expected |
|---|---|
| shrink/grow | `shrink_rewraps_long_line`: 3x10, `abcdefghijkl`, cols 5 -> rows `abcde` `fghij` `kl` (first two `WRAPPED`), `lines()` unchanged, `reflowed`, `rows_trimmed 0` |
| | `grow_joins_wrapped_rows`: same, cols 20 -> one row `abcdefghijkl`, not wrapped |
| | `line_ends_never_join`: `ab` CRLF `cd`, cols 20 -> two rows |
| | `empty_lines_stay_one_row_each`: 3 CRLFs between text survive a shrink and a grow |
| | `trailing_empty_trimmed_styled_blanks_kept`: bg-styled spaces after `ab` wrap on shrink like text |
| | `grapheme_and_extras_ids_survive`: `e\u{301}` and a hyperlinked cell keep their content/extras ids |
| wide pairs | `wide_pair_never_split`: 1x6 `abcd\u{FF21}`, cols 5 -> row 0 `abcd` + `LeadingWideSpacer` (`WRAPPED`), row 1 `Wide, WideSpacer`; cols 6 -> one row, no spacer |
| | `leading_spacer_dropped_on_grow`: wrapped-at-edge wide glyph, grow -> glyph right after the text |
| | `one_column_degrades_wide`: cols 1 -> blank cells, `dropped_wide` +1 per glyph |
| cursor | `cursor_follows_its_char`: 3x10 `abcdefghijkl` (cursor after `l`), cols 5 -> row of `kl`, col 2; cols 20 -> col 12 |
| | `pending_wrap_on_exact_fit`: `abcdefghij` at 10 cols (armed); cols 5 -> last row, col 4, armed; cols 20 -> col 10, not armed |
| | `cursor_keeps_distance_from_text`: `ab`, cursor to col 7; cols 4 -> 2 rows, cursor (row 2 of line, col 3) |
| | `saved_cursor_maps_like_cursor`; `cursor_line_trimmed_goes_to_newest` (sb 0, 2x10, cursor on row 0, `abcdefghij` on row 1, cols 1) |
| anchors | `mark_follows_its_char`; `mark_on_trimmed_blanks_lands_at_line_end`; `selection_anchors_map_like_marks` |
| | `mark_in_trimmed_history_dies` (`rows_trimmed > 0`, `get == None`, `kind` still `Some`); `screen_owned_never_die` |
| rows | `grow_pulls_history_first` (4x10, 10 lines; rows 6: history -2, cursor index +2, all ids kept) |
| | `grow_without_history_appends_blank_rows` (new ids above `newest`, `None` slots) |
| | `shrink_cursor_low_pushes_into_history`; `shrink_cursor_high_drops_blank_rows` (history same, `newest` lower) |
| | `shrink_keeps_content_below_cursor` (5 rows, cursor row 2, text on row 4; rows 3 -> rows 2..4 kept, cursor index 0) |
| | `released_id_comes_back_stamped` (drop, then LF re-issues the id: allocated, seq = that batch) |
| | `region_reset_on_resize` (also cols-only); `tab_stops_resize` (20 cols, clear stop 8, cols 10, cols 30: 8 clear, 16 and 24 set) |
| bounds | `size_is_clamped` (0x0 -> 1x1, `u16::MAX` -> 1024x2048); `same_size_is_a_noop` (default outcome, no seq bump) |
| alt | `alt_clips_never_reflows` (ids kept, rows truncated, primary reflowed); `alt_pending_wrap_rules`; `primary_reflows_while_alt_active` |
| scrollback | `zero_scrollback_reflow` (sb 0, shrink: excess counted in `rows_trimmed`, `history_len 0`) |
| | `max_scrollback_trim` (sb 20 full, cols halved: `history_len 20`, `rows_trimmed` = output - 20 - rows) |
| policy | `bottom_anchor_grow_pulls_history`; `keep_viewport_top_grow_appends_blank` (screen top id and cursor index unchanged) |
| | `keep_viewport_top_matches_host_row` (top row continues a history line; cursor index == host row measured before) |
| | `view_at_bottom_stays_at_bottom`; `scrolled_back_view_keeps_first_char` (both policies) |
| outcome | `outcome_rows_only_not_reflowed`; `outcome_counts_shift_trim` |

## `grid/screen/reflow_tests.rs` (child of `reflow.rs`, private access)

- `fast_path_keeps_row_buffers`: 4x20, short lines, history full; cols 10: every row buffer address
  after is in the set before; scratch `len 0`, capacity counted by `heap_bytes`.
- `full_history_shrink_recycles`: sb 50 full of 20-col lines, cols 10 twice back and forth: the
  number of buffer addresses never seen before is at most the output rows beyond recycled ones.
- `host_row_ignores_history_and_rows_below`; `lay_out_spans` (span table for a wide-pair line).
- `row_tests.rs`: `fit_truncates_and_pads`, `fit_repairs_edge_pairs`, `fit_occ_rule`.
- `anchor_tests.rs`: `kill_keeps_slot_not_freed`.

## Proptests (`grid/resize_props.rs`, child of `terminal_grid.rs`)

**`grow_then_shrink_back`**: size 4..8 x 4..12, sb 64; ops: print strings from
`['a', ' ', '\u{FF21}', '\u{301}', 'Z']`, CRLF, goto, wrapline, mark; policy either; grow by
`dr 0..4`, `dc 0..8` (not both 0), then resize back. `prop_assume!` both `rows_trimmed == 0`.

| Case | Promise |
|---|---|
| any | lines from oldest through the cursor's line equal (cell-exact, trailing `EMPTY` trimmed); cursor `(line, offset, pending)` equal; marks on content cells keep `(line, offset)` |
| `BottomAnchor` | also every line below the cursor, ignoring trailing empty lines |
| rows only, `BottomAnchor`, cursor on the bottom row | every live row `(id, cells, wrapped)` and the cursor equal; `offset` equal when it was 0 |
| rows only, `KeepViewportTop` | same exact equality, any cursor row |

**`no_row_id_collision`**: sequences of ops and resizes (1..10 x 1..12, both policies, alt
enter/leave). After each step: `check_integrity(Some)` ok; every id a reflow issued is greater
than every id live before it; `newest_issued` never decreases; lanes disjoint; live anchors in lane.

## Bench (deferred)

Tier 4 `resize` (80x24 -> 100x40 and back at 10,000 scrollback rows, `grow_us`/`shrink_us`) lands
with `vt-bench grid` in phase 6, per the roadmap. No bench in this phase.

## User chapter `docs/user/resize.md` (row 07; coder writes, <= 120 lines, all doctests)

1. Example: `TerminalGrid::new`, write a wrapped line, `resize(Size{..}, ResizePolicy::BottomAnchor)`,
   assert `outcome.reflowed` and the row text at the new width.
2. Choosing a policy: table of the two variants and when to use each (PTY vs ConPTY).
3. What reflow keeps: logical lines, wide glyphs, styles/links; trailing blanks; the alt screen
   is clipped; `cols == 1`.
4. Ids and anchors: column change = fresh ids; follow content with anchors; what dies.
5. `ResizeOutcome`: `reflowed`, `rows_trimmed`; checklist: resize the PTY too, redraw fully.
