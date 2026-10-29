# Tests

Unit tests beside the code (existing `*_tests.rs` files); integration tests only through the
public API. Every step adds its tests in its own commit. Names are suggestions.

## Existing tests that change

| Test | Step | Change |
|---|---|---|
| `events::event_tests::clear_shrinks_past_the_soft_caps` | S5 | events against `EVENT_LIST_SOFT` (32,768), params against `PARAM_LIST_SOFT` (4,096) |
| `tests/feed_limits.rs::a_hundred_thousand_bells_in_one_feed_do_not_pin_the_event_list` | S5 | `event_capacity() <= 32_768` |
| `tests/feed_limits.rs::a_forwarded_flood_of_separators_is_bounded_with_large` | S5 | `param_capacity() <= 4096` stays; adjust only if it read the shared constant |
| `grid/screen/screen_tests.rs` narrow-run helpers | S2, S4 | `print_run` and the `(char, u8)` closure |
| `tests/feed_stats.rs::grapheme_table_exhausted` | S7 | feed the 270,000 clusters in ONE `feed` (exhaustion now happens within a feed only); also assert the refused cells still show their CJK base |
| `intern_tests.rs` sweep tests that keep more than half a small arena | S7 | expect the half-cap rule |

## New tests

| Step | Test | Asserts |
|---|---|---|
| S1 | `intern_tests::lookup_counts_nothing` | `lookup` hit/miss; `entries`, `exhausted`, `needs_sweep` unchanged |
| S1 | `intern_tests::room_counts_free_and_unused` | fresh table 65,535; after a sweep frees k: `room` grows by k |
| S1 | `terminal_tests::styles_are_reclaimed_when_the_table_fills` | 80x4, scrollback 0: 70,000 distinct `CSI 48:2::r:g:b m` + `x` + CR LF; `style_table_exhausted == 0`; the last printed cell resolves to its colour |
| S1 | `terminal_tests::pens_survive_a_style_sweep` | set style A, DECSC, switch to alt screen with style B, fill the table with 70,000 one-off SGRs (nothing printed), DECRC / leave alt: pens still resolve to A and B; `CSI 2K` fills with A's background |
| S1 | `terminal_tests::a_full_live_set_still_falls_back` | 160x45, scrollback 500: 70,000 distinct styles each on a printed cell; `style_table_exhausted > 0`, debug integrity walk passes |
| S1 | `row_tests::a_freed_style_id_is_unknown` | `check_cell_ids` on a cell with a swept style id -> `UnknownStyle` |
| S2 | `row_tests::write_ascii_releases_both_edges` | spacer at the left edge blanks its wide cell; `Wide` at the right edge blanks its spacer; interior pairs overwritten; `occ`, `STYLED`/`HAS_EXTRAS` from the pen; `WRAPPED` cleared when the last column is written |
| S2 | `cell_tests::with_ascii_keeps_blank_encoding` | `with_ascii(b' ') == base`, `with_ascii(b'a').content() == Scalar('a')`, other fields kept |
| S2 | `screen_tests::ascii_run_stops_at_the_row_end` | 4 columns, "abcdef": 4 bytes, pending wrap; the next print wraps |
| S2 | `screen_tests::ascii_run_bails_on_a_wide_half_at_col_0_or_1` | filler above released exactly as the per-char path does |
| S2 | `terminal_tests::line_drawing_set_skips_the_ascii_path` | `ESC ( 0` then `qqq`: line-drawing glyphs |
| S3 | `parser_tests::ascii_prefix_block_edges` | lengths 0, 15, 16, 17, 31, 33; a `0x1F`, `0x7F`, `0x80` at each block offset |
| S3 | `parser_tests::mixed_runs_split_at_c0` | `"ab\u{e9}c\r\u{85}d"` and a cut multi-byte char at the chunk end: same log as before (per-scalar `Print`, `Execute`) |
| S4 | `screen_tests::wide_glyph_in_run_stops_before_the_last_column` | 5 columns, "中中中": two placed, third via the general path with a `LeadingWideSpacer` |
| S4 | `screen_tests::wide_glyph_overwrites_pairs_in_run` | existing pairs under narrow/wide writes stay well formed (integrity walk) |
| S4 | `parser_tests::quiet_prefix_finds_c1_after_a_block` | `C2 85` at offsets 15, 16, 31 -> `Execute(0x85)` |
| S5 | `row_tests::recycle_same_width_equals_new` | a used spare (occ 3) recycled == `Row::new(id, cols, seq, template)` |
| S6 | `screen_tests::region_scroll_across_the_ring_end` | scrollback 0 (1,024-slot ring), feed LFs until the region window straddles slot 1023/0; scroll up and down by 1 and 3: same text and stamps as a model of the expected rows |
| S6 | `screen_tests::slice_rotation_stamps_every_moved_row` | after a region scroll, every row of the region has the batch `seq` and `DIRTY` |
| S7 | `intern_tests::sweep_keeps_half_newest_first` | small arena (4 entries cap): live order `[d, c, b, a]` keeps `d, c`; `content(b)` is `Scalar(first of b)`; `get(b) == GraphemeId(0)`; `kept() == 2` |
| S7 | `intern_tests::sweeps_stay_amortised_when_live_exceeds_half` | after a sweep at the half cap, `needs_sweep()` stays false until `kept` new entries |
| S7 | `intern_tests::intern_joined_equals_intern_of_the_concatenation` | ids equal; truncation at 16 counted once |
| S7 | `terminal_tests::a_mark_run_interns_once` | `"a\u{300}\u{301}\u{302}"`: arena grows by exactly 1 entry |
| S7 | `terminal_tests::marks_split_across_feeds_give_the_same_cell` | the same cluster text whether the marks come in one feed or three |
| S8 | `terminal_tests::ascii_run_under_2027_extends_its_last_letter` | `?2027h` + `"abce"` + `"\u{301}"` in one feed and in two feeds: last cell `é`, width 1 |
| S8 | `terminal_tests::single_shift_applies_to_the_first_cluster_only` | `ESC N` before an ASCII word under `?2027` |
| S8 | `terminal_tests::rep_after_an_ascii_run_under_2027` | `CSI 2 b` repeats the last letter |
| S8 | `width_tests::cluster_str_width_matches_cluster_width` | for the `cluster_width` table cases |

## Chunk invariance (`tests/feed_chunking.rs`)

Add pieces: a 40-byte ASCII word (crosses the 16-byte blocks), `"\u{300}\u{301}\u{302}"`,
`"\x1b[48:2::1:2:3m"`, `"\x1b[2;4r"`, `"\x1b[?1049h"`, `"\x1b[?1049l"`, `"\x7f"`, `"\u{85}"`,
`"\x1b(0q\x1b(B"`. The property already compares rows, cursor, title, line count and events.

## Compare verify (manual, by the coder after S1 and after S8)

```bash
cargo build --release --manifest-path compare/Cargo.toml -p compare-driver
cargo build --release --manifest-path compare/engines/oneterm/Cargo.toml   # adapter only
target/release/compare.exe verify --engine oneterm
```

Expected: `csi_mix` ok; the other 20 golden fixtures ok; `combining_unique` and
`unicode_symbols` unverified (no golden). Paste the status lines into `acceptance.md`.
