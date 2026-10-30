# Tests

Unit tests beside the code (existing `*_tests.rs` files); integration tests only through the
public API. Every step adds its tests in its own commit. Names are suggestions.

## Existing tests that change

| Test | Step | Change |
|---|---|---|
| `screen_tests::slice_rotation_never_revives_a_stale_row` | S7 | delete: it plants a row of another id in a live slot, a state S7's invariant excludes; replaced by `integrity_flags_a_row_of_another_id_in_a_live_slot` |
| `screen_tests::slice_rotation_builds_rows_never_written` | S7 | keep the screen-text and `seq` asserts; they now hold through `RowRef::moved_at` before the restamp |
| tests asserting alternate-screen `RowsTrimmed` / `rows_scrolled` for a partial top region | S8 | none found in the suite; the new tests below pin the new behaviour |

## New tests

| Step | Test | Asserts |
|---|---|---|
| S1 | `terminal_tests::rep_equals_sending_the_copies` | for `c` in `a`, `中`, `q` under `ESC ( 0`, `n` in 1, 7, 255, 256, 300, 65535 (and 0, which means 1), with insert on and off, autowrap on and off, 10 columns: `c` + `CSI n b` gives the same rows, cursor and pending wrap as `c` sent `n + 1` times |
| S1 | `terminal_tests::rep_single_shift_applies_to_the_first_copy` | `ESC * 0` (G2 = line drawing), `q`, `ESC N`, `CSI 3 b`: the copies use GL, not G2; equal to sending them |
| S1 | `terminal_tests::rep_of_a_mark_attaches_each_copy` | `e`, `U+0301`, `CSI 2 b`: same cluster text as `e` + three `U+0301` sent one by one |
| S2 | `terminal_tests::a_background_only_pen_is_its_own_erase_style` | `CSI 48;5;3 m`: `styles.entries()` grows by 1; `CSI 2K` fills the row with that style id; `CSI 1;48;5;3 m` grows it by 2 |
| S3 | `intern_tests::sweep_sizes_the_index_once` | after a sweep keeping `k` clusters, `index.capacity() >= 2 * k` and no capacity change while `k` new clusters are interned |
| S4 | `intern_tests::colliding_hashes_chain` | intern `x`; point `index[cluster_hash("y")]` at `x`'s id; intern `y`: a new id whose `next` is `x`'s; `intern("x")` and `intern("y")` return their own ids; `resolve` of both is right |
| S4 | `intern_tests::many_unique_clusters_round_trip` | 100,000 distinct `a` + 3 marks clusters: distinct ids, each `resolve`s to its text, re-intern returns the same id |
| S4 | `intern_tests::sweep_keeps_lookups_working` | after a sweep, a kept cluster interns to its new id without growing the arena |
| S5 | `parser_tests::csi_split_at_every_byte` | for `ESC [ ? 1 ; 2 : 3 h`, `ESC [ 38 ; 2 ; 1 ; 2 ; 3 m`, `ESC [ 1 \x08 2 H`, `ESC [ 1 ? 2 h`, `ESC [ 1 $ p`, `ESC [ 40 x 1 ; ... m` (more than `MAX_PARAMS`), `ESC [ 99999 A`: every split point gives the same log as the whole |
| S5 | `parser_tests::escape_bracket_at_the_chunk_end` | `ESC` alone, then `[1m`; and `ESC [` alone, then `1m`: one `csi` call each, same params |
| S5 | `parser_tests::short_run_before_escape_prints_once` | `ab ESC [ m`, `abcdefghij ESC [ m` (look-ahead and `memchr` sides): one `print_str` with the text, then the `csi` |
| S6 | `row_tests::set_wide_updates_occ_and_hints_once` | `set` of a styled `Wide` at col 3: `occ` 5, `STYLED`, spacer at 4 |
| S6 | `screen_tests::glyph_run_releases_only_its_edges` | row `[W S n W S W S]`; run of 4 narrow + wide from col 1: the `Wide` at 0 is blanked, the spacer at the run end blanked, interior pairs overwritten, integrity walk passes |
| S6 | `screen_tests::glyph_run_equals_glyph_by_glyph` | proptest: random row content (narrow, wide pairs, leading spacer), random cursor, random text of narrow and wide scalars: `print_glyph_run` gives the same cells, `occ`, hints, cursor and pending wrap as writing each glyph with `set` |
| S6 | `screen_tests::wide_glyph_at_the_last_column_keeps_the_pair_under_nowrap` | `?7l`, a pair at cols 8-9 of 10, cursor at 9, print `中`: the pair is intact (the general path drops the glyph) |
| S7 | `screen_tests::moved_rows_read_the_batch_stamp_before_the_restamp` | region scroll: every region row's `row(id).seq()` is the batch, `row(id).id()` is `id`, text as expected; stored headers still old (`slots` inspected) |
| S7 | `screen_tests::the_next_batch_restamps_moved_rows` | after `set_seq`: every slot of a live id in the old range holds a row with that id and the old batch stamp; `moved` empty |
| S7 | `screen_tests::a_moved_blank_row_reads_changed` | scroll a region whose rows were never written over a written row: the blank arriving there reads `seq` = batch and `DIRTY`, and after the restamp is a built row with that stamp |
| S7 | `screen_tests::print_into_a_moved_row_writes_that_row` | scroll, then print on the vacated and on a moved row in the same batch: text and stamps right, no extra allocation (spare reused) |
| S7 | `screen_tests::integrity_flags_a_row_of_another_id_in_a_live_slot` | plant a row of id `x + ring` in the slot of screen row `x` (outside `moved`): `check_rows` returns `RowIdMismatch { id: x }` |
| S7 | `screen_tests::set_scrollback_limit_restamps_first` | region scroll, then a limit change that re-slots the ring: every row lands in its own slot |
| S7 | `snapshot_tests::region_scroll_between_updates_copies_moved_rows` | snapshot, region scroll in one feed, snapshot: changed rows reported, cells equal the screen |
| S8 | `terminal_tests::alt_partial_top_region_scrolls_in_place` | `?1049h`, `CSI 1;22 r`, 30 `y\r\n`: events are only `RowsScrolled { delta: -1 }` over rows 0..21 (plus `Repaint`), `rows_scrolled` 30 and `lines_produced` +30 as before, alternate `newest` unchanged, text equal to before; `CSI 2 S` there adds 2 to both counters |
| S8 | `terminal_tests::alt_full_screen_scroll_still_pushes` | `?1049h`, 50 LF: `RowsTrimmed` per LF and `rows_scrolled` 50, as today |

## Chunk invariance (`tests/feed_chunking.rs`)

Add pieces: `"x\x1b[300b"`, `"\x1b[48;5;9m"`, `"\x1b[?1;2:3h"`, `"\x1b["` (split bracket),
`"\u{4E2D}\u{4E2D}a\u{4E2D}"`, `"a\u{300}\u{301}\u{302}"` with unique marks per piece,
`"\x1b[?1049h\x1b[1;5r"`. The property already compares rows, cursor, title, line count
and events, so S8 stays covered for any chunking.

## Compare verify (manual, by the coder after S4, S7 and S8)

```bash
cargo build --release --manifest-path compare/Cargo.toml -p compare-driver
cargo build --release --manifest-path compare/engines/oneterm/Cargo.toml
target/release/compare.exe verify --engine oneterm
```

Expected: the 21 golden fixtures ok; `combining_unique` and `unicode_symbols` unverified.
The prototype's dumps were byte-identical to HEAD's on all 23 fixtures (evidence.md).
