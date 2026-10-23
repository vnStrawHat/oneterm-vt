# Tests

Unit tests drive a real `Terminal` through `feed` with byte sequences (no grid internals), in
`snapshot/snapshot_tests.rs` and `snapshot/sync_tests.rs`. Fixture (first file, `pub(super)`):
`Session { term, batch, state, now }` with `feed(&[u8])`, `update() -> SnapshotUpdate`,
`update_at(Instant)`. Default size 10 x 20 unless stated. One behaviour per test.

## Update decisions (`snapshot_tests.rs`), one test per state.md result row

| Test | Steps -> assert |
|---|---|
| `first_update_is_full` | new state -> `Full`, 10 rows, `changed` = `0..10` |
| `idle_update_is_unchanged` | update twice -> `Unchanged`, `changed` empty, `rows_copied` flat over 5 idle updates |
| `rows_hold_the_viewport_for_every_result` | `Full`, `Unchanged`, `Partial` -> `rows().len() == 10` each |
| `one_written_row_is_partial` | `ESC[4;1Hhello` -> `Partial { scrolled: 0 }`, `changed == [3]`, `rows_copied` +1 |
| `one_line_scroll_is_a_move` | fill 10 rows, `ESC[10;1H\n` -> `Partial { scrolled: 1 }`, `changed == [9]`, `rows()[0].id` = old `rows()[1].id` |
| `two_line_scroll` | 8 x 20, two LFs on the last row -> `Partial { scrolled: 2 }`, `changed == [6, 7]`, kept ids shifted by 2 |
| `scroll_past_the_viewport_is_full` | 15 LFs on the last row -> `Full` |
| `viewport_scroll_back_is_negative` | 20 LFs, update, `scroll_viewport(-5)` -> `Partial { scrolled: -5 }`, `changed == [0, 1, 2, 3, 4]`, `scroll_offset() == 5`; `scroll_viewport(i32::MAX)` -> `Partial { scrolled: 5 }` |
| `viewport_scroll_past_the_viewport_is_full` | 30 LFs, `scroll_viewport(-12)` -> `Full` |
| `output_while_scrolled_back_is_not_copied` | scrolled back 5, write on the cursor row -> `Partial { scrolled: 0 }`, `changed` empty |
| `resize_is_full` | `resize(12 x 20, BottomAnchor)` -> `Full`, 12 rows, `size() == 12 x 20`; `size()` is `0 x 0` before the first update |
| `alt_screen_enter_and_leave_are_full` | `ESC[?1049h` -> `Full`; `ESC[?1049l` -> `Full` |
| `ris_decaln_deccolm_reverse_video_are_full` | each of `ESC c`, `ESC#8`, `ESC[?3h`, `ESC[?5h` -> `Full`; a second `ESC[?5h` -> no bump (`Unchanged`) |
| `scrollback_limit_change_is_full` | `set_scrollback_limit(10)` -> `Full` |
| `color_override_change_is_full` | `ESC]4;1;rgb:ff/00/00 BEL` -> `Full`; the same OSC again -> `Unchanged` (no change, no bump) |
| `theme_change_is_full` (D2) | `set_theme_colors(&other)` -> `Full`; the same theme again -> `Unchanged` |
| `invalidate_is_full_once` | `invalidate()` -> `Full`, then `Unchanged` |
| `every_row_copied_is_full` | `ESC[2J` after content -> `Full` |
| `state_from_another_terminal_is_full` (D1) | state built on a terminal fed 50 batches, then used on a fresh one -> `Full`, no debug panic |
| `cursor_move_is_partial_without_rows` | `ESC[5;8H` -> `Partial { scrolled: 0 }`, `changed` empty, `cursor().row == Some(4)`, `col == 7` |
| `cursor_hidden_while_scrolled_back` | scrolled back past the cursor -> `cursor().row == None` |
| `selection_change_is_partial_once` | `selection_start` + `selection_update` -> `Partial`, `changed` empty, `selection()` = `selection_range()`; next -> `Unchanged` |
| `mode_change_is_partial_without_rows` | `ESC[?1h` -> `Partial`, `modes().app_cursor`; `ESC[?25l` -> `Partial`, `cursor().visible == false` |
| `insert_mode_write_is_partial` | `ESC[4h`, write on row 2 -> `changed == [2]` |
| `region_scroll_restamps_rows` | `ESC[2;5r`, `ESC[5;1H\n` -> `Partial { scrolled: 0 }`, `changed == [1, 2, 3, 4]`, row 4 blank |
| `watermark_never_moves_backwards` | 10 feeds + updates: `watermark()` non-decreasing; an idle update keeps it |
| `two_states_keep_their_own_damage` | two states, one write: both `Partial` with `changed == [6]`, then both `Unchanged` |

## Rows, palette, modes (`snapshot_tests.rs`)

| Test | Assert |
|---|---|
| `style_runs_carry_resolved_styles` | `ESC[31mred` -> `runs[0] == 0..3` with `fg == Named(Red)`, 2 runs |
| `uniform_row_is_one_run` | 50 x 200, `ESC[42m ESC[2J` -> every row one run `0..200` |
| `style_of_bad_run_is_default`, `cluster_out_of_range_is_empty` | `Style::DEFAULT`; `&[]` for `(u32::MAX, u32::MAX)` |
| `grapheme_cell_copies_as_cluster` | `e` + U+0301 -> `Cluster`, `cluster(..) == ['e', '\u{301}']` |
| `wrapped_row_is_flagged` | 21 chars on 20 cols -> `rows()[0].wrapped`, not `rows()[1]` |
| `hyperlink_strings_resolved` | OSC 8 `id=1` link -> cell `hyperlink` set, `hyperlink(id).uri` matches; unknown id -> `None` |
| `graphics_hook_is_empty` | `placements()` empty; every `graphic` `None`; `graphic_offset(GraphicId(1), ..) == None` |
| `palette_defaults` | `indexed[1] == cd0000`, `[16] == 000000`, `[196] == ff0000`, `[232] == 080808`, `[255] == eeeeee` |
| `palette_resolve_and_dim` | named / indexed / rgb; dim pins `{103,0,0}` and `{123,20,30}`; `DimForeground`, `BrightForeground` fallbacks |
| `map_colors_is_idempotent` | twice -> same runs; `fg == Rgb(palette.foreground)` |
| `epoch_change_remaps_every_row` | OSC 4 change, `Full`, `map_colors(&themed)` -> every row themed |
| `same_epoch_remaps_changed_rows_only` | map with A; write row 3; map with B -> only row 3 carries B (the documented contract) |
| `effective_palette_precedence` (D4) | OSC 4;1 wins over the theme; OSC 104;1 restores it; OSC 10 sets `foreground` |
| `mode_snapshot_default_is_power_on` | `ModeSnapshot::default() == Terminal::new(..).mode_snapshot()` |
| `mode_snapshot_fields_follow_modes` | one sequence per field (`?1049`, `?1`, `ESC =`, `?2004`, `?25`, `4`, `?1007`, `?5`, `CSI > 1 u`, `CSI > 4 ; 2 m`) |
| `mouse_protocol_derivation` | `?1000h` -> Normal/Default; `?1002h ?1006h` -> ButtonEvent/Sgr; `?9h` -> X10; `?1003h ?1015h` -> AnyEvent/Urxvt; `?1005h` alone -> `None`; `?1003h` then `?1000l` -> AnyEvent; `mouse_reporting() == mode_snapshot().mouse` |

## Allocation steadiness (`snapshot_tests.rs`)

- `steady_state_grows_no_buffer`: 50 x 200; 600 cycles of (write a line on row `cycle % 50`,
  update, `map_colors`); per-row `(cells, runs, clusters)` capacities and the `changed` and
  `hyperlinks` capacities at cycle 60 equal those at the end.
- `idle_updates_copy_nothing`: 100 idle updates -> `rows_copied` unchanged (also in the table).

## Sync (`sync_tests.rs`, `SYNC_REFRESH`/`SYNC_WATCHDOG` imported)

| Test | Assert |
|---|---|
| `block_suppresses_until_close` | `?2026h` + write row 2 -> `Unchanged` at `+REFRESH/2`; `?2026l` -> `Partial`, `changed == [2]` |
| `refresh_deadline_resumes_frames` | no `l`: at `+REFRESH` -> `Partial`, `changed == [1]` |
| `watchdog_forces_the_mode_off` | `h` refreshed every `REFRESH/2` -> `Unchanged` until `+WATCHDOG`; then `Partial`, `changed == [3]`, `!sync().is_set()`, DECRQM `?2026` answers `2` |
| `decrqm_reports_the_real_state` | `CSI ? 2026 $ p` -> `ESC[?2026;2$y`; after `h` -> `;1$y`; after `l` -> `;2$y` |
| `first_update_in_a_block_builds` | `h` before the first update -> `Full`, 10 rows; next -> `Unchanged` |
| `mode_change_in_a_block_reaches_the_next_frame` | `?25l` inside: `Unchanged`; after `l`: `Partial`, `changed` empty; then `Unchanged` |
| `cursor_move_in_a_block_reaches_the_next_frame` | same with `ESC[5;10H`: `cursor() == (Some(4), 9)` |
| `modes_refresh_while_suppressed` | `?2004h ?1h` inside -> `Unchanged` and `modes()` already shows both |
| `ris_ends_the_block`, `decstr_keeps_it` | `ESC c` -> `!is_set()`; `CSI ! p` -> `is_set()` |
| `deadline_reports_the_next_wake` (D5) | closed `None`; after `h` at `t` -> `t + REFRESH`; refreshed at `t + 0.9 s` -> `t + WATCHDOG` |
| `absurd_clock_does_not_panic` | `begin` at an `Instant` near the maximum (via `checked_add` probing) -> no panic, no suppression |

`Terminal: Send + Sync` stays asserted (0007). No timing test in `vt` (tier 3 measures cost).

## User chapters (doctests, `#[cfg(doctest)]` includes)

- **`docs/user/snapshot.md`** (new, row 18): example (feed, `snapshot_update`, walk `rows()`
  with `style_of`/`cluster`); the three results and what a renderer does for each (cache shift
  by `scrolled`, `changed()`); `rows_copied`, `invalidate`, one state per consumer and per
  terminal; `map_colors` contract and theme vs override precedence (`set_theme_colors`,
  `palette()` or the `colors()` merge); `ModeSnapshot` and `MouseProtocol`; hyperlinks; the
  placement hook (phase 10).
- **`docs/user/threading.md`** (row 02): who owns the lock; the two-phase hand-off diagram;
  `Repaint` is a hint, coalesce on a frame timer; `?2026` behaviour, timeouts, `deadline()`,
  the embedder checklist (sync.md).
- **`docs/user/quickstart.md`** (row 01): `Terminal::new`, `feed`, drain events, one
  `snapshot_update`, print `rows()` as text; asserts the first row's text.
