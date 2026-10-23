# Tests

Each test checks one behaviour. Sibling files per module (`#[cfg(test)] mod row_tests;` etc.).
`grid/grid_fixture.rs` (`#[cfg(test)] mod grid_fixture;` in `grid/mod.rs`): `Fixture { grid:
TerminalGrid, interner: Interner }`, `fixture(rows, cols)` (scrollback 100), `fixture_with(.., sb)`,
helpers `print(&str)` (per char, `scalar_width`), `print_with(PrintMode, &str)`, `newline()` (CR +
LF), `goto`, `set_style(Style)`, `row_text(id)`, `index_text(index)`, `label_rows()` (`r0`, `r1`, ..).
**Every mutating helper ends with `check()`** = `assert_eq!(grid.check_integrity(Some(&interner)),
Ok(()))`; tests that call grid ops directly call `f.check()` after each one.

## `row_tests.rs`

| Test | Assert |
|---|---|
| `new_row_is_template_dirty_occ_zero` | cells = template, `occ 0`, flags `DIRTY \| hints` |
| `set_tracks_occ_and_hints` | set col 4 styled -> `occ 5`, `STYLED`; grapheme -> `HAS_GRAPHEME`; extras -> `HAS_EXTRAS` |
| `set_wide_places_its_spacer` | `Wide` at 2 -> 3 is `WideSpacer` with the same style; `Wide` at the last col -> blank `Narrow` |
| `set_releases_the_overwritten_pair` | over a `Wide` -> its spacer blank; over a `WideSpacer` -> its `Wide` blank |
| `set_degrades_stray_spacers` | `WideSpacer` anywhere and `LeadingWideSpacer` inland -> blank `Narrow`; `LeadingWideSpacer` at last col kept |
| `write_reaching_last_column_clears_wrapped` | `set_wrapped(true)`; set col < last keeps it; set last col clears it; `repair` at last col keeps it |
| `reset_fast_path_is_exact` | reset A, write 3 cells, reset A -> all A; reset B -> all B; `occ 0` both times |
| `recycled_row_carries_nothing` | dirty styled wrapped row with extras -> `recycle` -> same buffer pointer, all `EMPTY`, flags `DIRTY` only, len = cols |
| `delete_chars_shifts_left` | `abcdefghij`, DCH 3 at 2 -> `abfghij   `; DCH 40 at 2 -> `ab        ` |
| `insert_cells_pushes_wide_off_the_end` | `Wide` at `cols - 2` + ICH 1 at 0 -> no `Wide` without spacer; last col blank |
| `edits_repair_pairs_at_their_edges` | EL/ECH range ending on a `Wide` or starting on a `WideSpacer` -> both halves blank |
| `unwritten_row_reads_blank` | `RowRef` blank: not allocated, `occ 0`, flags empty, `cell(9999) == EMPTY` |
| checker tests | paranoid.md list |

## `screen/screen_tests.rs` (drive through the fixture)

| Group | Tests (expected values) |
|---|---|
| ids, viewport | `offset_zero_follows_output` (4x10, 10 newlines: offset 0, history 7); `scrolled_back_view_holds_still` (`scroll_viewport(-3)` then 2 newlines: offset 5, same `visible_top`); `offset_capped_by_history` (-10,000 -> `history_len`, top = `oldest`; `scroll_to_bottom` -> 0); `ids_never_rewind` (scroll, ED 2, ED 3, RIS: `newest` never decreases); `ring_len_rounds_up` (10,000 -> 16,384; 0 -> 1,024; `u32::MAX` -> 1,048,576); `unwritten_slots_read_as_blanks`; `trimmed_slot_cleared_before_reuse` (2x10, sb 2, `ring_len + 5` newlines: every live row blank with its own id); `history_stays_within_limit` |
| cursor, region | `set_region_validates_and_homes` (1..4 ok, homes; 4..4 refused, region and cursor kept; `Region` origin homes to index 2 for 2..5); `goto_origin_clamps_to_region`; `linefeed_below_region_stops_at_bottom`; `il_dl_outside_region_are_noops`; `backspace_clears_pending_wrap`; `backspace_at_col_zero_keeps_pending_wrap`; `save_restore_cursor_round_trip` |
| scrolling | `scroll_up_feeds_history_only_from_row_zero`; `bounded_region_keeps_rows_below` (5 rows labelled, region 0..3, n 1: `r1 r2 _ r3 r4`, history 1 = `r0`); `small_region_rotates_then_blanks` (region 1..3, n 9: `r0 _ _ r3 r4`, no new id); `scroll_down_never_pulls_from_history`; `dl_at_row_zero_feeds_history` |
| print | `pending_wrap_then_el0_erases_nothing` (autowrap on and off); `autowrap_reenabled_over_pending_wrap_breaks_line` (`abcde` off -> `abce`; `X` on -> row 1 `X   `, cursor (1, 1)); `tab_with_pending_wrap_wraps_and_is_consumed`; `tab_with_autowrap_off_is_consumed_in_place`; `wide_at_last_column` (4 cols, col 3: `LeadingWideSpacer` + `WRAPPED`, next row `Wide, WideSpacer`; autowrap off: dropped, `dropped_wide 1`, not wrapped); `wide_wrapping_over_a_pair`; `tab_keeps_a_spacer_a_spacer`; `cross_row_spacer_release_keeps_wrapped`; `insert_mode_over_wide_repairs`; `zero_width_at_col_zero_attaches_there` (`" \u{301}    "`, cursor col 0); `wide_dropped_when_one_column`; `control_chars_are_ignored` |
| erase | `ed2_scrolls_occupied_rows_into_history` (`hello` -> history 1, screen blank); `ed2_keeps_scrolled_back_view`; `ed2_keeps_image_rows` (graphic extras on row 2 -> history 3); `ed2_alt_blanks_without_history`; `ed1_clears_row_zero` (`  bbb     `); `ed3_resets_offset_and_reports_trimmed`; `erased_cells_keep_only_background` (ECH, DCH, ICH, EL 2, ED 0 with fg/bg/underline pen -> `EMPTY.with_style(bg only)`); `row_reset_respects_occ_and_template` |
| damage | `blanking_stamps_dirty_with_batch_seq` (scroll and reset paths; seq = new batch); `seq_starts_at_one_and_grows` |
| memory | `empty_scrollback_costs_only_its_ring` (45x160, sb 10,000, 10,045 LFs: 0 rows allocated; `size_of::<Option<Row>>() == size_of::<Row>()`; `heap_bytes` in `[ring * slot, ring * slot + 4096)`); `no_allocation_when_scrolling_full_history` (below) |

**`no_allocation_when_scrolling_full_history`** (no `unsafe`, no allocator hook): 4x10, sb 8, run
twice (default pen; pen with a background colour). Warm up with 20 cycles of "goto bottom row,
print `x`, LF". Record `heap_bytes()` and the set of row buffer addresses (`cells.as_ptr() as
usize` over `slots` and `spare`, read directly: the test is a child of `screen`). Run 1,000 more
cycles; after each: `heap_bytes()` equal, every buffer address in the recorded set, ring pointer
unchanged. Second part: 6 rows all written, region 1..5, 100 x `scroll_up(region, 1)` and
`scroll_down`: same two assertions. Allocation in these paths would show as a new address.

## `anchor_tests.rs` and `terminal_grid_tests.rs`

| Test | Assert |
|---|---|
| `region_scroll_moves_anchors_with_content` | mark at 3, region 1..5 up 2 -> index 1; mark at 5 stays |
| `scrolled_out_anchor_dies` | up: mark at 1 dies; down: mark at 4 dies |
| `trim_kills_anchors` | 200 full scrolls, sb 100 -> mark dead |
| `rows_scrolled_matches_anchor_shift` | region 1..5 up 2: `delta -2`, top/bottom ids, `history_rows 0` |
| `whole_screen_scroll_reports_nothing` | `scrolled None`, `history_rows 2`, mark unchanged |
| `bounded_top_region_reports_tail_shift` | region 0..4 up 2: tail mark `+2`, `delta 2`, still index 5 |
| `alt_scroll_leaves_primary_anchors` | mark + selection on primary; enter alt, LF; both alive, unchanged |
| `erase_keeps_anchors_alive`, `wipe_kills_screen_anchors`, `released_id_is_recycled` | as named |
| alt screen | `enter_keeps_cursor_index`; `leave_restores_entry_cursor`; `region_and_tabs_shared_both_ways`; `wipe_uses_entering_background`; `enter_leave_are_idempotent`; `lanes_never_overlap` (20 LFs each) |

## Proptest `grid_props.rs` (child of `terminal_grid`)

`ROWS 6, COLS 8, SCROLLBACK 12`; glyphs `['a', ' ', '\u{FF21}', '\u{301}', 'Z']`; ops vec `1..40`:
print / print-at-col (glyph, insert, autowrap), LF, RI, wrapline, scroll up/down (top `0..6`,
bottom `0..=6`, n `0..8`), IL, DL, EL 0-2, ECH, DCH, ICH (`0..12`), ED 0-3, goto, set_region,
tab `0..3`, `scroll_viewport(-4..4)`, enter/leave alt, wipe, mark (register `Mark`), release, RIS.
Per step: `begin_batch`, op, `sync_anchors`, `check_integrity(Some)` == `Ok`; every live row
whose cells or `wrapped` changed carries this batch's seq and `DIRTY`; anchors on the inactive
screen stay alive unless the op is enter/leave/RIS/release. End: `offset <= history_len <= 12`;
primary range ends before the alt range. Default case count.

## User chapter `docs/user/grid.md` (row 16, coder writes, <= 120 lines, all doctests)

1. Example: `TerminalGrid::new(Size { rows: 3, cols: 10 }, 100)`; write `hello` into the cursor
   row with `screen_mut().row_mut(id)` (an `Option`) and `set`; `wrapline()` 3 times; `history_len() == 1`;
   `row_text(oldest, &Interner::default().graphemes, &mut s)` starts with `hello`.
2. Screen vs viewport: `scroll_viewport`, `visible_top`, `scroll_to_bottom`; primary vs alternate.
3. Rows and ids: `RowId` is a position, never reset, two lanes; follow content with anchors,
   never by storing a `RowId`. `RowRef`: `cells`, `cell`, `wrapped`, `is_allocated`.
4. Scrollback: `DEFAULT_SCROLLBACK`, `SCROLLBACK_MAX`, ring cost (48 B/slot, power of two),
   unwritten rows cost nothing; `oldest`, `newest`, `row_range`, `history_len`.
5. Reading checklist: resolve style/grapheme ids through the terminal's `Interner`; skip spacer
   cells; `SeqNo` is per batch.
