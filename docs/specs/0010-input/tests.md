# Tests

Unit tests beside the code: `input/key_tests.rs`, `input/kitty_tests.rs`, `input/mouse_tests.rs`
(`#[cfg(test)] #[path = "..._tests.rs"] mod ..._tests;`, the crate's convention). Snapshots are
built in-crate as `ModeSnapshot { field, ..ModeSnapshot::default() }`. Expected bytes are
written with `\x1b` escapes, never raw control bytes in source. One behaviour per test.

## Golden tables (roadmap acceptance: byte-exact per (event, snapshot) pair)

Each is one `#[test]` iterating a `const`/`static` table that mirrors the topic file's byte table
row for row (a row with several events becomes several entries). A failure message names the row.

| Test | File | Table |
|---|---|---|
| `legacy_table` | `key_tests.rs` | legacy.md: control table (every listed scalar), named-key table (every cell: plain, plain + `app_cursor`, each modifier in {Shift, Ctrl, Alt, Ctrl+Alt}), worked rows |
| `modify_other_keys_table` | `kitty_tests.rs` | modify-other-keys.md byte table, each row at levels 0, 1, 2 |
| `kitty_table` | `kitty_tests.rs` | kitty.md byte table |
| `mouse_table` | `mouse_tests.rs` | mouse.md byte table |

## Unit tests (70; one behaviour each)

**`key_tests.rs` (32):** `every_named_key_is_known_here` (exhaustive `match`, 38 keys, each
encodes), `only_app_cursor_is_read`, `ctrl_c_is_0x03`, `ctrl_space_is_nul`,
`ctrl_uppercase_letter_uses_same_control_byte`, `ctrl_punctuation_and_digits_follow_xterm_table`,
`ctrl_unmapped_ascii_passes_through`, `ctrl_non_ascii_has_no_encoding`,
`ctrl_alt_letter_prefixes_esc_to_control_byte`, `ctrl_alt_bracket_prefixes_esc_to_esc`,
`enter_plain_cr`, `enter_shift_csiu`, `backspace_plain_del`, `backspace_ctrl_bs`,
`arrow_up_plain`, `arrow_up_app_cursor`, `arrow_up_ctrl`, `tab_shift_backtab`, `home_plain`,
`home_app_cursor`, `home_ctrl_ignores_app_cursor`, `end_plain`, `end_app_cursor`,
`delete_ctrl_csiu_tilde`, `alt_char_prefix`, `f1_plain`, `f5_plain`, `f12_plain`,
`alt_arrow_up`, `alt_home`, `ctrl_pageup`, `alt_pageup`. A local test
wrapper `fn encode_key(key, mods, app_cursor: bool)` shadows the public one.

**`kitty_tests.rs` (15):** `specification_worked_examples`,
`enter_tab_and_backspace_survive_a_crashed_program`,
`the_three_legacy_keys_have_no_release_without_report_all`,
`release_is_silent_and_repeat_is_a_press_by_default`,
`app_cursor_is_not_read_once_the_kitty_rung_applies`,
`disambiguate_covers_only_the_keys_that_produce_no_text`,
`alternate_keys_and_text_are_inert_on_their_own`,
`the_text_field_is_dropped_rather_than_sent_unsafely`, `an_empty_character_has_no_kitty_encoding`
(**D3**: expects `None` for all 32 flag sets, plain and Alt), `modify_other_keys_levels`,
`kitty_wins_over_modify_other_keys`, `shifted_function_keys_take_the_private_use_codes`,
`f3_never_collides_with_the_cursor_position_report` (widened, below),
`a_text_key_has_no_event_types_without_report_all_keys`, `the_text_fallback_respects_the_modifiers`.

**`mouse_tests.rs` (23):** `x10_reports_a_press_and_nothing_else`,
`urxvt_is_decimal_and_has_no_column_ceiling` (its hover row uses `AnyEvent`, **D4**),
`sgr_press_left`, `sgr_release_left`, `sgr_press_with_ctrl`, `x11_press_left`,
`x11_release_uses_button_3`, `wheel_up_sgr`, `wheel_down_sgr`, `move_hover_code_35` (`AnyEvent`,
D4), `x11_press_with_ctrl`, `x11_press_with_shift_alt`, `x11_coordinates_above_127_are_raw_bytes`,
`x11_coordinates_beyond_byte_range_are_capped`, `utf8_mouse_encodes_large_coordinates_as_utf8`,
`utf8_mouse_small_coordinates_match_legacy`, `utf8_mouse_caps_at_two_byte_form`,
`sgr_wins_over_utf8`, `x11_coordinates_at_boundary_95`, `sgr_preserves_large_coordinates`,
`sgr_release_right_with_modifiers`, `wheel_x11_mode`, `move_hover_x11` (`AnyEvent`, D4).

## New tests

| Test | File | Asserts |
|---|---|---|
| `function_keys_carry_modifiers` | key | D1: Ctrl+F1 `\x1b[1;5P`, Shift+F3 `\x1b[13;2~`, Ctrl+F5 `\x1b[15;5~`, Alt+F12 `\x1b[24;3~` |
| `insert_carries_modifiers` | key | D1: Alt+Insert `\x1b[2;3~`, Ctrl+Insert `\x1b[2;5~` |
| `f13_to_f24_ignore_modifiers` | key | Ctrl+F13 `\x1b[1;2P`, Alt+F15 `\x1b[28~` |
| `alt_prefixes_single_byte_named_keys` | key | D2: Alt+Enter, Alt+Tab, Alt+Shift+Tab, Alt+Escape, Ctrl+Alt+Backspace |
| `empty_character_sends_nothing` | key | D3: `encode_key(Character(""))` plain, Alt, Ctrl -> `None` |
| `no_output_is_a_cursor_position_report` | kitty | widens the F3 test: 38 keys x 8 modifier sets x 32 flag sets x levels 0-2; no `Some` is `\x1b[` .. `R` |
| `motion_follows_reporting` | mouse | D4: the four reporting modes x {move Left, hover} per mouse.md first table |
| `wheel_without_vertical_delta_sends_nothing` | mouse | D5: `0.0`, `-0.0`, `f64::NAN` -> empty |
| `x10_ignores_the_encoding` | mouse | `?9` with `Sgr`/`Utf8`/`Urxvt` -> legacy bytes |
| `encode_key_reads_the_terminals_modes` | terminal tests | feed `\x1b[?1h` -> ArrowUp `\x1bOA`; `\x1b[>1u` -> Escape `\x1b[27u`; `\x1b[<u\x1b[>4;2m` -> Ctrl+`a` `\x1b[27;5;97~`; `\x1bc` -> `\x01` |
| `encode_key_event_reports_a_release` | terminal tests | feed `\x1b[>10u`; `a` Release -> `\x1b[97;1:3u` |

Rustdoc examples (doctests): `KeyEvent` (builder, `text == None`), `encode_key_event` (Escape
before and after `\x1b[>1u`), `encode_key` (Ctrl+`c`), `encode_mouse_press` (`?1000` + `?1006`).

If a design choice is ruled B, its new test is deleted and the unit-test fixture takes the B value.

## `docs/user/input.md` (<= 120 lines; every Rust block a doctest)

1. Intro: pure encoders; the engine knows the modes; the embedder owns side effects (scroll to
   bottom, clear selection, shortcuts). Map platform events to `KeySpec`/`KeyMods`/`NamedKey`.
2. Keys example: `Terminal::encode_key` for ArrowUp and Ctrl+`c`; free `encode_key` with
   `mode_snapshot()`; `None` = drop the event.
3. The ladder (four rungs) and which `ModeSnapshot` field each reads; DECCKM.
4. Legacy highlights: Alt = ESC prefix, modified function keys, F15 and the CPR rule.
5. Kitty flags table (one line each) and `modifyOtherKeys` levels; example `\x1b[>1u` -> Escape
   `\x1b[27u`; `KeyEvent` example (`\x1b[>10u`, release `a` -> `\x1b[97;1:3u`); what to fill
   (`shifted`, `base_layout`, `text`) and the ceilings (three modifiers, 38 keys, PC-101 un-shift).
6. Mouse example (`?1000` + `?1006`, press -> `\x1b[<0;1;1M`); `Terminal::hit_test` for
   coordinates; reporting and encoding tables; empty = write nothing; `mouse_reporting() == None`
   = the event is the embedder's (selection, scroll); ceilings 223/2015.
7. Embedder checklist: wrap pastes when `bracketed_paste`; wheel -> arrow keys via `encode_key`
   when `alt_screen && alternate_scroll && mouse.is_none()`; focus `CSI I`/`CSI O` when
   `mode(Mode::FocusInOut)`; write the bytes to the PTY outside the lock.
