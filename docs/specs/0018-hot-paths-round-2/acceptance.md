# Acceptance: steps S1-S8

Machine i7-12700, Windows 11, release profile. Each number is `vt-bench grid --fixture NAME --mib 32`
(median of 5 cycles after a warm-up), pinned to logical CPU 4 at high priority, one run per
fixture and step unless a cell says "best of 3" (interleaved, before and after). The first
column is the code before S1 (the spec commit).

## Commits

| Step | Subject |
|---|---|
| spec | `docs(spec): second round of engine hot paths` |
| S1 | `perf(terminal): repeat REP copies through the print run, csi_mix 52 -> 122 MiB/s` |
| S2 | `perf(grid): reuse a background-only pen as its erase style, dense_cells 188 -> 213 MiB/s` |
| S3 | `perf(intern): size the grapheme sweep index once, combining_unique 20 -> 24 MiB/s` |
| S4 | `perf(intern): key graphemes by hash chain, no boxed key, combining_unique 21 -> 46 MiB/s` |
| S5 | `perf(parser): run CSI headers in a local loop, cursor_motion 241 -> 310 MiB/s` |
| S6 | `perf(grid): write glyph runs with edge-only pair release, cjk_wide 219 -> 366 MiB/s` |
| S7 | `perf(grid): restamp rotated rows once per batch, scroll_region 341 -> 538 MiB/s` |
| S8 | `perf(grid): scroll a partial alternate-screen top region in place, scroll_region_small 35 -> 58 MiB/s` (owner approved) |

## Thresholds (files.md)

| Step | Fixture | Before | After | Floor | Met |
|---|---|---:|---:|---:|---|
| S1 | `csi_mix` | 51.6 | 122.3 | 105 | yes |
| S2 | `dense_cells` | 187.6 | 213.0 | 205 | yes |
| S3 | `combining_unique` | 18.1 (best of 3: 20.2) | 20.6 (best of 3: 23.8) | 21 | yes, best of 3 |
| S4 | `combining_unique` | 20.6 | 46.3 | 40 | yes |
| S5 | `cursor_motion` | 241.3 | 310.4 | 285 | yes |
| S5 | `tui_redraw` | 327.3 | 367.9 | 355 | yes |
| S5 | `heavy_sgr` | 255.3 | 307.8 | 280 | yes |
| S5 | `vim_session` | 166.7 (best of 3: 207.7) | 271.0 | 240 | yes |
| S5 | `csi_mix` | 124.1 | 164.5 | 145 | yes |
| S5 | `long_osc` | 879.9 | 851.4 | >= 0.95 x before (836) | yes (0.967) |
| S6 | `cjk_wide` | 218.6 | 365.7 | 310 | yes |
| S6 | `unicode_symbols` | 222.1 | 353.6 | 310 | yes |
| S6 | `long_lines` | 848.6 | 1025.0 | 900 | yes |
| S7 | `scroll_region` | 340.8 | 537.9 | 475 | yes |
| S7 | `scroll_region_small` | 25.8 | 34.9 | 31 | yes |
| S8 | `scroll_region_small` | 34.9 | 58.1 | 50 | yes |

## Every fixture after each step (MiB/s)

| Fixture | Before | S1 | S2 | S3 | S4 | S5 | S6 | S7 | S8 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `heavy_sgr` | 250.7 | 259.9 | 260.9 | 242.7 | 255.3 | 307.8 | 303.4 | 304.3 | 294.2 |
| `dense_cells` | 188.1 | 187.6 | 213.0 | 209.1 | 212.7 | 240.0 | 239.8 | 237.0 | 227.9 |
| `tui_redraw` | 310.5 | 314.3 | 316.6 | 293.2 | 327.3 | 367.9 | 352.0 | 337.1 | 346.3 |
| `cursor_motion` | 229.6 | 231.1 | 231.4 | 195.1 | 241.3 | 310.4 | 303.4 | 300.6 | 297.2 |
| `sixel` | 42.9 | 40.4 | 37.0 | 36.2 | 42.9 | 42.8 | 42.6 | 37.1 | 37.5 |
| `hyperlinks` | 271.3 | 268.0 | 272.8 | 268.8 | 275.5 | 278.3 | 276.3 | 279.0 | 246.7 |
| `grapheme_clusters` | 57.1 | 57.3 | 57.5 | 51.6 | 57.2 | 57.0 | 58.0 | 53.5 | 52.7 |
| `scroll_region` | 340.4 | 295.8 | 346.1 | 330.1 | 349.2 | 339.3 | 340.8 | 537.9 | 515.1 |
| `plain_ascii` | 641.9 | 660.3 | 644.2 | 630.8 | 655.5 | 681.4 | 783.6 | 757.1 | 636.4 |
| `csi_mix` | 51.6 | 122.3 | 122.4 | 120.1 | 124.1 | 164.5 | 169.0 | 170.3 | 164.9 |
| `combining_unique` | 14.5 | 17.9 | 18.1 | 20.6 | 46.3 | 47.8 | 47.5 | 43.2 | 44.8 |
| `vim_session` | 206.3 | 193.7 | 206.3 | 204.4 | 166.7 | 271.0 | 265.4 | 256.3 | 261.2 |
| `mode_churn` | 147.8 | 149.9 | 143.2 | 144.2 | 125.3 | 213.4 | 200.9 | 188.9 | 201.6 |
| `cjk_wide` | 214.9 | 217.0 | 215.3 | 208.0 | 211.0 | 218.6 | 365.7 | 331.0 | 340.2 |
| `unicode_symbols` | 218.9 | 222.7 | 221.4 | 198.7 | 218.2 | 222.1 | 353.6 | 359.3 | 330.9 |
| `long_lines` | 729.8 | 842.0 | 767.1 | 695.0 | 684.5 | 848.6 | 1025.0 | 1023.1 | 858.7 |
| `scroll_region_small` | 26.3 | 26.1 | 25.7 | 24.8 | 26.9 | 26.0 | 25.8 | 34.9 | 58.1 |
| `long_osc` | 871.1 | 904.1 | 890.1 | 888.0 | 879.9 | 851.4 | 850.6 | 889.5 | 893.6 |

## Guard fixtures (below 0.95 x the column before)

The machine drifts by up to 15 % between runs (`sixel` spreads 15 % within one run). Each
single-run drop was re-measured, before and after interleaved, best of 3. None held.

| Step | Fixture | Single run | Interleaved best of 3 (before / after) | Verdict |
|---|---|---|---|---|
| S1 | `scroll_region`, `sixel`, `vim_session` | 340.4 to 295.8, 42.9 to 40.4, 206.3 to 193.7 | 363.9 / 363.1, 43.1 / 43.1, 211.2 / 211.1 | noise, off the path |
| S2 | `sixel`, `long_lines` | 40.4 to 37.0, 842 to 767 | 40.0 / 42.7, 799.8 / 836.5 | noise, off the path |
| S3 | `cursor_motion`, `unicode_symbols`, `long_lines`, `grapheme_clusters` | 231 to 195, 221 to 199, 767 to 695, 57.5 to 51.6 | 233.8 / 231.9, 223.0 / 222.6, 823.4 / 839.1, 58.5 / 58.4 | noise, off the path |
| S4 | `vim_session`, `mode_churn` | 204.4 to 166.7, 144.2 to 125.3 | 207.7 / 214.0, 148.4 / 153.3 | noise |
| S6 | `tui_redraw` | 367.9 to 352.0 (0.957) | 355.3 / 345.3 (0.972) | within 5 % |
| S6 | `mode_churn` (not a guard) | 213.4 to 200.9 | 212.3 / 195.4 (0.92); aligned build 212.0 / 213.6; `tui_redraw` aligned 385.6 / 380.2 | code placement, not work |
| S7 | `sixel`, `grapheme_clusters`, `cjk_wide` | 42.6 to 37.1, 58.0 to 53.5, 365.7 to 331.0 | 41.7 / 41.6, 57.5 / 56.7, 362.6 / 370.0 | noise |
| S7 | `tui_redraw` | 352.0 to 337.1 (0.957) | 354.3 / 337.9 (0.954) | within 5 % (S7 is not on its path) |
| S8 | `hyperlinks`, `plain_ascii`, `long_lines`, `scroll_region` | 279 to 247, 757 to 636, 1023 to 859, 538 to 515 | 277.4 / 273.4, 766.6 / 750.0, 1022.3 / 1017.0, 574.3 / 566.7 | within 5 % |

## Gate (every step)

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo test --workspace` and `cargo test --workspace --all-features`: green after each step.

| Step (counts as first landed) | Tests (default / all features) |
|---|---|
| before S1 | 1182 / 1191 |
| S1 | 1185 / 1194 |
| S2 | 1186 / 1195 |
| S3 | 1187 / 1196 |
| S4 | 1190 / 1199 |
| S5 | 1193 / 1202 |
| S6 | 1198 / 1207 |
| S7 | 1204 / 1213 |
| S8 | 1206 / 1215 |
| final, after the review follow-up | 1216 / 1225 |

After the review follow-up: `cargo test --workspace --no-default-features` 1216 passed and clippy clean;
`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` clean;
`scripts/public-api.sh check` exit 0 (snapshot unchanged); `compare verify --engine oneterm`:
21 fixtures ok, `combining_unique` and `unicode_symbols` unverified (as expected).

## Deviations

- Measured values are single runs; the floors are met in the single run except S3, where
  `combining_unique` (a fixture with a 15 % spread) was decided on the interleaved best of 3.
- S8 landed after the owner approved it, so `compare verify`, `--no-default-features`, doc
  and public-api ran after S8 (and again after the review follow-up). `README.md` now records
  the approval and ticks the criteria except the `compare run` one (S9).
- `ESC [` at the very end of a chunk enters `CsiEntry` directly (the prototype's form) and the
  next chunk continues the header; the log equals the whole-input log, which the S5 tests
  assert. Only a lone `ESC` stays in `Escape`.
- Test names differ slightly from the suggestions: `rep_single_shift_applies_to_the_first_copy_only`;
  `glyph_run_releases_only_its_edges` starts its run on the spacer at column 2, because the
  guard that declines a wide-pair half at columns 0 and 1 would decline the spec's column 1;
  `glyph_run_blanks_the_spacer_it_orphans_at_its_end` is an extra; `short_run_before_escape_prints_once`
  counts `print_str` calls with a small local sink.
- `a_background_only_pen_is_its_own_erase_style` pins the style table growth. The change saves a
  lookup and gives the same ids, so no behaviour test can fail without it.
- Not done: S9 (`compare run` and its commit) and any README or `docs/perf` change.

## Review follow-up (folded into the step commits)

- **S7, disjoint windows:** a second rotation apart from the first now settles the first
  (`restamp_moved`) before `moved` widens, so the rows between two regions are not reported
  changed, allocated or copied. Tests: `two_disjoint_region_scrolls_leave_the_rows_between_untouched`
  and the snapshot test `two_disjoint_region_scrolls_copy_only_their_rows` (`changed()` is exactly
  rows 1-3 and 19-23).
- **S7, trimmed moved ids:** `moved` is kept among the live ids by clamping its start when
  history is trimmed (`forget_moved_below_oldest`), so a trimmed id reads as a plain blank
  (`a_trimmed_moved_id_reads_as_a_plain_blank`). A liveness test in `stored` and `row` cost
  `scroll_region` about 4 % (583 -> 554), so the invariant is kept at the trim instead.
- **S7 tests added:** ED 3 after a region scroll in one batch; a slice rotation followed by a
  ring-end rotation in one batch.
- **S2, S1, S5, S8 tests:** the erase-style test checks template and erase ids, the three
  non-background SGR forms and `CSI 49 m`; REP asserts `q`-`U+2500`-`qq` directly, adds SS3 and
  the consumed-shift case, wrapping under `?7h`/`?7l`/`?2027h`, and a dropped wide glyph under
  `?7l`; the CSI split test adds five malformed headers; S8 adds `CSI 1;24 r` (still pushes),
  `DL` at row 0 of a partial region, and `?1049l` leaving primary ids unchanged.
- **Not done:** sharing `printable_ascii_prefix` with the parser's `ascii_prefix`. Measured
  interleaved best of 3, `tui_redraw` 372.1 -> 334.6 (0.90) and `cursor_motion` 307.3 -> 292.7
  (0.95); the duplicate stays.
- **Pinned after the follow-up (interleaved best of 3, S8 build / final build):**
  `scroll_region` 580.4 / 567.1 (0.977), `scroll_region_small` 57.8 / 58.5,
  `tui_redraw` 352.3 / 354.3, `cursor_motion` 300.3 / 300.6.
- `vt-bench grid --check`: PASS, 14 of 14 fixtures above 0.50 x the committed baseline.
- The compare driver and all 8 adapters rebuilt with `cargo build --release`; `compare verify
  --engine oneterm` 21/21 ok.
