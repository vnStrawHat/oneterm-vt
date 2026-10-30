# Tests

Unit tests beside the code (existing `*_tests.rs` files); integration tests only through the
public API. Every step adds its tests in its own commit. Names are suggestions.

## Existing tests that change

| Test | Step | Change |
|---|---|---|
| `anchor_tests::set_revives_but_leaves_released_ids_alone` | S5 | plants a `Cursor` anchor below `oldest` and kills it with `trim`, which now returns early (no other anchor). Kill it with `kill(a)`; the revive and release asserts stay |
| `event_tests::finish_lossy_repairs_invalid_utf8` | S2 | delete with `EventBatch::finish_lossy`, whose only caller was `text_span` |
| `row_tests::recycle_*` (3 tests) | S6 | `Row::recycle(Some(Box::new(spare)), ..)`; compare `*recycled` with `Row::new` |
| `screen_tests` that plant rows (5 sites), `reflow_tests` scratch type | S6 | `Some(Box::new(row))`; `Vec<Option<Box<Row>>>` |
| `screen_tests` ring-size test, `terminal_grid_tests::heap_bytes_cover_both_rings_and_the_anchors`, `resize_tests::reflow_scratch_is_empty_between_calls_and_counted` | S6 | `size_of::<Option<Box<Row>>>()` (8, `== size_of::<usize>()`) instead of `size_of::<Option<Row>>()` |

With those edits the prototype (all six steps) passes `cargo test -p oneterm-vt
--all-features` (1,072 tests) except the planned anchor test and the two `user_guide` tests
that need the docs tree (absent in the scratch copy); clippy `-D warnings` is clean.

## New tests

| Step | Test | Asserts |
|---|---|---|
| S1 | `parser_tests::osc_split_at_every_byte` | for `ESC ] 8 ; id=x ; http://a;b BEL`, `ESC ] 0 ; t ESC \`, `ESC ] 2 ; a \x01 b BEL` (ignored C0 inside), 20 `;`-separated params, a 3,000-byte payload, `ESC ] 5 2 ; c ; <3 KiB> BEL` with `osc_allows_large(52)`, `ESC ] 2 ; x CAN y`: every split point gives the same log (code, params, terminator, `truncated`) as the whole |
| S1 | `parser_tests::dcs_split_at_every_byte` | `ESC P q #0;2;0;0;0 ~~-~ ESC \`, the same with a `DEL` and a `0x85` inside, with `CAN` in the middle: the same `dcs_hook` / `dcs_put` sequence / `dcs_unhook(aborted)` for every split |
| S1 | `parser_tests::escape_bracket_osc_at_the_chunk_end` | `ESC` alone then `]0;t BEL`; `ESC ]` alone then `0;t BEL`: one `osc` call each, same params |
| S1 | `osc::tests::extend_never_grows_an_unclaimed_osc` | `extend` with 64 KiB slices, 20 times: capacity stays `OSC_INLINE`, length `OSC_INLINE`, `truncated` |
| S1 | `osc::tests::extend_grows_a_claimed_osc_by_doubling_to_large` | claimed code; `extend` 9 x 1 MiB: length `OSC_LARGE`, capacity `<= OSC_LARGE`, `truncated`; `start` shrinks back |
| S2 | `intern_tests::explicit_link_keys_keep_boundaries_apart` | `intern(Some("a"), "bc")` and `intern(Some("ab"), "c")`: two ids; each re-intern returns its id; `len()` 2 |
| S2 | `intern_tests::a_known_explicit_link_adds_nothing` | intern the same `(id, uri)` 1,000 times: one id, `len()` 1, the 4 MiB budget charged once (a 4 MiB - 10 byte link then fits exactly as before) |
| S2 | `osc_tests::invalid_utf8_in_a_link_and_a_notification_is_replaced` | OSC 8 URI and OSC 9 body with `\xFF` inside: `U+FFFD` in the link URI and in the `Notification` body, same as before |
| S2 | `events::event_tests::text_span_returns_the_text` | `text_span("é;x")` reads back the same `&str`; an empty `text_span("")` is empty |
| S3 | `sixel_tests::raster_first_image_hands_over_its_buffer` | `"1;1;5;3 #1~`: width 5, height 3, `rgba.len() == 60`, `rgba.capacity() == 60`, column 0 painted, the rest transparent |
| S3 | `sixel_tests::paint_wider_than_the_stride_narrows` | no raster, 70 columns painted (stride doubles to 128): width 70, every row's pixels in place |
| S3 | `sixel_tests::paint_past_the_raster_is_cut` | raster 4x6, 10 columns and 3 bands painted: width 4, height 6, pixels of columns 0-3 of the first band only |
| S3 | `sixel_tests::raster_after_paint_widens` | 3 columns painted, then `"1;1;100;6`: width 100, painted pixels at their place |
| S3 | `sixel_tests::finish_equals_the_copy_loop` | proptest over random strings of `0-9 ; # ! " $ - ? ~` and printable sixels: `finish` equals a test-only copy of HEAD's `finish` (allocate, copy `min(width, stride)` columns of the first `height` rows) on width, height, `band_pixels` and `rgba` |
| S4 | `terminal_grid_tests::live_style_ids_read_an_erased_tail` | `CSI 41 m`, `CSI K` at column 3 of a row with text: the erase style id is in `live_style_ids` although every cell carrying it is at `col >= occ` |
| S4 | `terminal_grid_tests::live_extras_ids_read_only_the_used_prefix` | a row with a linked cell at column 2 and `occ` 3: the link's extras id is found; a stale id in no cell is not |
| S5 | `anchor_tests::trim_and_shift_skip_above_the_lowest_follower` | only screen-owned anchors: `trim` and `shift_region` change nothing; a `Mark` at row 10: `trim(0, 5)` keeps it, `trim(0, 11)` kills it; `set` it to row 3 after a walk raised the watermark: `trim(0, 4)` kills it again (the watermark followed `set`); `shift_region` by -5 over it lowers the watermark with it |
| S5 | `screen_tests::full_screen_linefeed_equals_a_region_scroll` | two screens fed the same rows, 5-line history limit: 30 LFs at the bottom through `linefeed` vs `scroll_up(full region, 1)`: same reports, rows, ids, cursor, saved cursor, `oldest`, anchors |
| S5 | `terminal_tests::linefeed_with_a_scrolled_viewport_keeps_its_offset` | scroll the viewport up 3, 10 LFs: the viewport stays on the same content (general path), `RowsTrimmed` as before |
| S6 | `screen_tests::heap_bytes_counts_boxed_rows` | after `n` written rows `heap_bytes` grows by `n * (size_of::<Row>() + cols * size_of::<Cell>())` |

## Chunk invariance (`tests/feed_chunking.rs`)

Add pieces: `"\x1b]8;id=a;http://x/y\x07"`, `"\x1b]9;7;s=1;t=b\x07"`, `"\x1b]2;a\x01b\x07"`,
`"\x1bPq\"1;1;4;2#1~~-~\x1b\\"`, `"\x1bPq#0;2;0;0;0!5~\x1b\\"`, `"\x1b]"`, `"\x1bP"`. The
property compares rows, cursor, title, line count and events for any chunking.

## Compare verify (manual, by the coder after S3, after S5 and after S6 if approved)

```bash
cargo build --release --manifest-path compare/Cargo.toml -p compare-driver
cargo build --release --manifest-path compare/engines/oneterm/Cargo.toml
target/release/compare.exe verify --engine oneterm
```

Expected: the 21 golden fixtures ok; `combining_unique` and `unicode_symbols` unverified.
The prototype's dumps were byte-identical to HEAD's on all 23 fixtures (evidence.md).
