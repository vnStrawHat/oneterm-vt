# Acceptance: steps S1-S5

Machine i7-12700, Windows 11, release profile. Method as files.md: `vt-bench grid --fixture NAME
--mib 32`, pinned to logical CPU 4 at high priority, median of 5 (MiB/s). Guard re-checks:
interleaved best of 3, "before" binary first. S6 (boxed ring rows) is not implemented: the owner
has not decided on it. `compare run` is not part of this commit.

## Per step: before -> after (pinned medians)

| Step | Fixture | Before | After | Threshold |
|---|---|---:|---:|---:|
| S1 | `long_osc` | 886.5 | 9,577 | >= 7,000 |
| S1 | `hyperlinks` | 269.1 | 901.1 | >= 680 |
| S1 | `osc_9_7` | 244.1 | 305.2 | >= 270 |
| S1 | `sixel`, `sixel_large` | 42.4, 24.0 | 44.8, 26.5 | none |
| S2 | `hyperlinks` | 901.1 | 1,209.8 | >= 1,100 |
| S2 | `osc_9_7` | 305.2 | 333.6 | >= 325 |
| S3 | `sixel_large` | 26.5 | 48.1 | >= 37 |
| S3 | `sixel` | 44.8 | 51.2 | >= 51 |
| S4 | `sixel` | 51.2 | 60.5 | >= 58 |
| S5 | `short_lines` | 57.5 (interleaved best of 3) | 63.0 (63.8 interleaved) | >= 63 |
| S5 | `osc_9_7` | 333.6 (after S2) | 357.4 | >= 350 |
| S5 | `sixel` | 60.5 | 71.1 | >= 69 |

All thresholds met. `sixel` after S3 (51.2) and `short_lines` after S5 (63.0) are at the floor:
the machine ran about 5 % slower than in the prototype's session (S2 binary `sixel` 43.6 vs 46
there).

## Guards (>= 0.95 x before)

Guard list: `heavy_sgr`, `dense_cells`, `tui_redraw`, `cursor_motion`, `csi_mix`, `plain_ascii`,
`scrolling`, `cjk_wide`, `grapheme_clusters`, `scroll_region`, measured before and after each step.
A single pinned run fell below 0.95 in three places; each held in the interleaved re-check, so the
drops are single-run noise (the steps do not touch those paths or the loop condition):

| Step | Fixture | Single run before / after | Interleaved before / after (best of 3) |
|---|---|---|---|
| S1 | `scrolling` | 1,207 / 920 | 1,179 / 1,167 (0.99) |
| S1 | `cjk_wide` | 370 / 315 | 373 / 373 (1.00) |
| S1 | `grapheme_clusters` | 57.0 / 51.8 | 55.5 / 56.3 (1.01) |
| S3 | `csi_mix` | 171.3 / 152.4 | 170.3 / 166.6 (0.98) |
| S3 | `scrolling` | 1,170 / 788 | 1,089 / 1,110 (1.02) |
| S3 | `cjk_wide` | 362 / 300 | 364 / 366 (1.01) |
| S5 | `scroll_region` | 568 / 502 | 565 / 555 (0.98) |

Other re-checks (interleaved): S1 `cursor_motion` 0.976 single run; S2 `scroll_region_small` 59.4
/ 58.5 (0.985), `long_osc` 9,359 / 9,289 (0.99), so the known S2 `scroll_region_small` drop did
not show up; S5 `scroll_region_small` 58.8 / 58.3, `tui_redraw` 362 / 352 (0.97). No aligned
build was needed.

## Final checks (after S5)

- Gate: `fmt --check`, `clippy --workspace --all-targets --all-features -D warnings`,
  `cargo test --workspace` (1,236 pass) and `--all-features` (1,245 pass): green after every step.
- `cargo clippy` and `cargo test -p oneterm-vt --no-default-features` (961 lib tests): green.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`: clean.
- `scripts/public-api.sh check`: unchanged (see the deviation about `Anchors::default`).
- `compare verify --engine oneterm`: 21 ok; `combining_unique` and `unicode_symbols` unverified.
- Sixel images (size, id and RGBA of every `take_graphics` image, FNV-1a 64): HEAD `1fb640c` and
  the final tree give identical hashes: `sixel` 479,350 images `567a89ca2098c19d`, `sixel_large`
  596 images `f0a6010d30c6a724`, 3,000 random Sixel strings (raster before/after/without paint,
  paint past the raster, repeats, `CAN` aborts) 2,561 images `b120921f0a17078c`.

## Deviations from the spec

1. `tests/feed_chunking.rs` (S1): the new Sixel and OSC pieces made the property fail on HEAD as
   well, because a `GraphicReleased` event is reported by whichever `feed` ends after the sweep, so
   its position and presence follow the chunking. `describe` leaves `GraphicReleased` out; all
   other events, rows, cursor, title and line count still compare equal.
2. `terminal_grid_tests::live_style_ids_read_an_erased_tail` (S4): `CSI K` with a styled pen sets
   `occ` to the row width (the fill raises it), so it never leaves a styled cell in the tail. The
   test scrolls rows in under a styled pen instead (new rows are reset to the erase cell, `occ` 0,
   every cell styled) and checks that only the last-cell read finds the id; removing the `chain`
   makes it fail.
3. `Anchors` (S5): the hand-written `Default` spells its return type `Anchors`, not `Self`, so the
   public-API snapshot text does not change.
4. `sixel_tests::finish_equals_the_copy_loop` (S3): the old copy loop runs over the new decoder's
   state (after the raster pre-allocation), not over HEAD's decoder. The HEAD-versus-now image
   hashes above cover the pre-allocation end to end.
5. Added tests beyond tests.md: `parser_tests::dcs_payload_past_the_cap_aborts_once` (the S1 cap
   arithmetic, whole and 4 KiB chunks) and `terminal_tests::linefeed_once_history_is_full_reports_one_trim_per_line`.
6. `compare verify` ran once, after S5 (not also after S3); the S3 image hashes stand in for it.
7. `linefeed_into_history` carries the `hotpath-profiling` attribute as the spec asks; the
   prototype did not.

## Review follow-up

The advisor's differential run (550 seeds against `1fb640c`) found no difference. Changes it
asked for, each folded into its step commit:

- S1: the chunking property no longer drops `GraphicReleased` unchecked. Each run ends with an
  empty `feed`, and the sorted union of released ids and live placement ids must be equal between
  the whole and the chunked run; per run, released ids are unique and none is live. The chunk
  dependence itself is legitimate (the pending ceiling and the sweep run per feed; HEAD behaves
  the same), now said in `docs/user/graphics.md`. `osc_split_at_every_byte` also covers an OSC
  with a C1 and a `DEL` byte inside and one ended by `ESC Z` (deviation 1 above is narrowed).
- S2: `explicit_key` writes the length with `to_string`, no `fmt::Write`. `hyperlinks`, interleaved
  best of 3, 1,267 / 1,222 and 1,169 / 1,176 (0.96, 1.01), inside the 5 % allowance.
- S3: `declare_raster` only records the raster; `reserve` allocates `width * height` on the first
  painted pixel, so `DCS q "1;1;4096;4096 CAN` allocates nothing (test
  `a_raster_alone_allocates_nothing`). New test for a second raster after the buffer exists
  (`"1;1;10;6#1~~"1;1;4;3` gives a 4x3 image with columns 0-1 painted).
- S5: `Screen::debug_check` asserts that the three screen-owned anchors are at or above `oldest`,
  the promise `Anchors::trim` and `shift_region` rely on when they return early.

Final tree, pinned median of 5: `sixel` 74.2 (thresholds: S3 51, S4 58, S5 69), `sixel_large`
52.4 (37), `hyperlinks` 1,183 (1,100). Interleaved against the pre-review binary: `sixel`
71.1 / 75.2, `sixel_large` 50.8 / 52.7, `osc_9_7` 357 / 361. Gate, `--no-default-features`, doc,
public API, `compare verify` (21 ok) and the Sixel image hashes were re-run on the final tree
with the same results (hashes unchanged).
