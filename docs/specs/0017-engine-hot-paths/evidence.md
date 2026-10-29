# Evidence: baseline, profiles, prototype

Machine i7-12700, Windows 11, rustc 1.96.0, release profile. "Pinned" = one process on
logical CPU 4 at high priority (`start /affinity 10 /high`), the loop of `vt-bench grid`
(64 KiB chunks, 160x45, 10,000 history lines, `default-features = false` as in `compare/`),
1 warm-up + 5 passes, median. Corpus: `vt-bench fixtures --compare --mib 32`.

## Profiles at HEAD (`hotpath-profiling` plus extra sites in a throwaway copy)

| Fixture | What the profile showed |
|---|---|
| `combining_unique` (4 MiB) | `sweep_graphemes` ran after 82 of 128 feeds, 46.6 ms each = 83 % of the time; `attach` 3.6 M calls (3 per cell); arena at the cap (262,144 entries), 884,715 clusters refused |
| `csi_mix` | style table at 65,061 entries after 4 MiB, full later; `style_table_exhausted` > 0; dump differs at checkpoint 0, `attrs[0]` expected `(0,0,9,"i113","r776270","")`, got the default style |
| `scroll_region` | `linefeed` 46 %, `scroll_up` 38 %, `rotate` 28 %, `print_narrow_run` 26 % |
| `scroll_region_small` | `linefeed` 61 %, `scroll_up` 49 %, `rotate` 25 % (24 tail rows per LF on the alt screen) |
| `short_lines` | `linefeed` 39 %, `print_text` 34 %; 21,845 `RowsTrimmed` events per feed against a list shrunk to 4,096 at every `clear` |
| `grapheme_clusters` | `print_text` 93 %, `attach` 3.3 M calls 33 %; every ASCII letter is its own `place_cluster` |
| parser alone (tier 1) | `plain_ascii` 1,148 MiB/s, `long_lines` 1,359, `cjk_wide` 1,017: ~0.8 ns/B of a 6.3 ns/B grid cost |

## Prototype, step by step (pinned MiB/s)

Cumulative, in prototype order S2, S5, S6, S3, S4, S1, S7, S8: each "before" is the state
reached by the rows above it.

| Step (files.md) | Fixture: before -> after |
|---|---|
| S2 bulk ASCII write, scan bounded by the row | `plain_ascii` 155 -> 408, `long_lines` 162 -> 552, `scrolling` 163 -> 572, `light_cells` 175 -> 681 |
| (S2 without the bound) | `light_cells` 175 -> 165: the ASCII scan re-read the whole 7,200-byte run at every row |
| S3 parser ASCII scan | `plain_ascii` 456 -> 627, `scrolling` 572 -> 811, `long_lines` 549 -> 857, `light_cells` 670 -> 1,245 |
| S4 wide glyphs in the run | `cjk_wide` 140 -> 205, `unicode_symbols` 147 -> 212 |
| S4 quiet control scan | `cjk_wide` 207 -> 227, `unicode_symbols` 214 -> 232 |
| S4 edge-only pair release (rejected) | `cjk_wide` 223 -> 231 (+3 %, within noise; not planned) |
| S5 event soft cap | `short_lines` 51 -> 58 (no scroll events at all: 61) |
| S6 slice rotation | `scroll_region` 169 -> 271, `scroll_region_small` 17 -> 27 |
| S1 style sweep, fill check before every SGR (rejected) | `heavy_sgr` 268 -> 250 (6 % lost on hits) |
| S1 style sweep, lookup first, sweep on a miss | `heavy_sgr` 266 / 265, `dense_cells` 190 / 188 (noise); `csi_mix` exhausted 0 |
| S7 one intern per mark run alone | `combining_unique` 2.1 -> 2.3 (the sweep still dominates) |
| S7 + half-cap sweep, dense remap | `combining_unique` 2.1 -> 17.4 (sweep 20 per 8 MiB x 18.6 ms) |
| S8 ASCII bulk + one attach per cluster | `grapheme_clusters` 27 -> 36 |
| S7/S8 key scratch + width without re-segmenting | `grapheme_clusters` 36 -> 57, `combining_unique` 17.4 -> 18.4 |

## All fixtures, HEAD vs the full prototype (pinned MiB/s)

| Fixture | HEAD | Proto | Fixture | HEAD | Proto |
|---|---:|---:|---|---:|---:|
| `plain_ascii` | 156 | 638 | `grapheme_clusters` | 24-27 | 57 |
| `long_lines` | 162 | 840 | `hyperlinks` | 245 | 251 |
| `heavy_sgr` | 262 | 260-265 | `mode_churn` | 142 | 154 |
| `tui_redraw` | 192 | 320 | `sixel_large` | 25.1 | 24.9 |
| `scroll_region` | 91 | 278 | `cursor_motion` | 227 | 240 |
| `cjk_wide` | 142 | 225 | `light_cells` | 176 | 1,205 |
| `dense_cells` | 191 | 188 | `short_lines` | 38 | 55 |
| `scrolling` | 162 | 828 | `scroll_region_small` | 10.5 | 27 |
| `sixel` | 45.0 | 45.3 | `csi_mix` | 55 (diverged) | 57 (ok) |
| `osc_9_7` | 161 | 230 | `combining_unique` | 1.1 (vt-bench) | 18.4 |
| `long_osc` | 765 | 895 | `unicode_symbols` | 151 | 232 |
| `vim_session` | 162 | 219 | | | |

Unpinned `vt-bench grid --mib 32` at HEAD for the record: `plain_ascii` 152, `short_lines` 37.5
(spread 49 %), `grapheme_clusters` 17.5 (53 %), `combining_unique` 1.1. Spreads that large are
why the thresholds in files.md are measured pinned.

## Prototype correctness

- The compare `oneterm` adapter built against the prototype: all 21 golden fixtures match,
  `csi_mix` included (it diverges at HEAD); `combining_unique` and `unicode_symbols` have no golden.
- The engine test suite on the prototype (final design): 864 unit tests, one failure
  (`clear_shrinks_past_the_soft_caps`, new constant); integration failures only where tests.md
  says they change: the two soft-cap tests in `feed_limits.rs` and `feed_stats::grapheme_table_exhausted`
  (270,000 clusters over 64 KiB feeds no longer exhaust: sweeps keep half the arena free).
