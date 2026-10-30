# Evidence: profiles, ablations, prototype

Machine i7-12700, Windows 11, rustc 1.96.0, release profile (thin LTO, line tables). "Pinned"
= one process on logical CPU 4 at high priority (`start /affinity 10 /high`), 160x45, 10,000
history lines, 64 KiB chunks, `default-features = false`, 1 warm-up + 5 passes, median. Corpus:
`.tmp/corpus/*.vt` (32 MiB each, the compare corpus v1). HEAD = `78d9020`.

## Tools (scratch, `.tmp/s18`, not part of the spec)

- Runner: the vt-bench grid loop, plus `PARSE=1` (parser alone into a null sink) and `SB=n`
  (scrollback limit) to split parser, grid and memory costs.
- Sampling profiler: a thread on CPU 6 suspends the feeding thread (CPU 4) every ~100 us and
  reads its instruction pointer (`SuspendThread` / `GetThreadContext`); addresses are mapped
  to functions through the linker map (`/MAP`) and to instructions with `dumpbin /disasm`.
  Samples in system DLLs land on the image's last symbol (`__dyn_tls_init_callback`).
- Ablation builds: one change that breaks behaviour but shows a cost (REP a no-op, restamp
  loop removed, ...). Prototype: branch `steps4` of the scratch repo, one commit per step S1-S8 (then the rejected
  width table); the S5 and S6 commits there are the measured final forms.

## Where the time goes at HEAD (pinned profiles and ablations)

| Fixture | Finding | Step |
|---|---|---|
| `csi_mix` | REP made a no-op: 63 -> 219 MiB/s; REP prints 100 copies one scalar at a time | S1 |
| `dense_cells` | `HashMap::get` 32 %: pen style and erase style looked up per SGR | S2 |
| `combining_unique` | `reserve_rehash` 19.5 %; system heap 35 % (boxed key per cluster, freed at sweeps) | S3, S4 |
| `cursor_motion`, `tui_redraw`, `heavy_sgr`, `dense_cells` | parser alone 40-60 % of grid time (parser.md table) | S5 |
| `cursor_motion` (after S5) | 1-letter print removed: 316 -> 459; CUP removed: 316 -> 355 | (left) |
| `cjk_wide` | `RowMut::set` 51 %, one `occ` load 43 % of it (store-forwarding stall); decode 25 %; `lookup_width` 9-14 % | S6 |
| `scroll_region` | restamp loop removed: 358 -> 612; `scroll_region_small` 26 -> 37 | S7 |
| `scroll_region_small` | in-place alternate region (S8 prototype): 33.4 -> 59.3 | S8 |
| `short_lines` | `SB=100`: 61 -> 71-76 (cold recycled rows); rest spread over ~10 functions, none > 25 % | (left) |

## Prototype per step (pinned MiB/s, cumulative, spec order)

| Fixture | HEAD | S1 | S2 | S3 | S4 | S5 | S6 | S7 | S8 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `cjk_wide` | 223 | 223 | 222 | 225 | 224 | 215 | 353 | 370 | 373 |
| `combining_unique` | 18.8 | 18.7 | 18.6 | 22.0 | 48.7 | 44.7! | 48.4 | 47.8 | 47.4 |
| `csi_mix` | 55.7 | 126 | 123 | 125 | 124 | 156 | 170 | 170 | 173 |
| `cursor_motion` | 244 | 236 | 234 | 236 | 235 | 308 | 307 | 300 | 302 |
| `dense_cells` | 194 | 190 | 216 | 214 | 217 | 233 | 239 | 238 | 240 |
| `grapheme_clusters` | 59.8 | 59.4 | 59.6 | 59.8 | 58.2 | 52.7! | 53.2 | 57.9 | 57.7 |
| `heavy_sgr` | 266 | 263 | 263 | 263 | 263 | 288 | 304 | 304 | 306 |
| `hyperlinks` | 283 | 280 | 282 | 281 | 282 | 260! | 276 | 281 | 278 |
| `light_cells` | 1277 | 1267 | 1278 | 1276 | 1250 | 1136! | 1787 | 1780 | 1730 |
| `long_lines` | 785 | 806 | 862 | 865 | 822 | 791 | 1036 | 1064 | 995! |
| `long_osc` | 887 | 854 | 881 | 911 | 883 | 892 | 885 | 899 | 906 |
| `mode_churn` | 157 | 154 | 156 | 154 | 153 | 210 | 211 | 198! | 209 |
| `osc_9_7` | 243 | 234 | 251 | 256 | 256 | 222! | 236 | 247 | 244 |
| `plain_ascii` | 671 | 677 | 626! | 678 | 668 | 591! | 787 | 761 | 776 |
| `scroll_region` | 361 | 364 | 356 | 366 | 351 | 346 | 343 | 539 | 578 |
| `scroll_region_small` | 26.0 | 26.3 | 25.5 | 25.6 | 25.3 | 27.9 | 26.3! | 33.0 | 58.5 |
| `scrolling` | 926 | 931 | 926 | 816! | 953 | 761! | 1246 | 1207 | 1230 |
| `short_lines` | 61.6 | 59.8 | 60.8 | 60.0 | 60.6 | 60.6 | 59.7 | 60.8 | 58.4 |
| `sixel` | 45.1 | 41.9! | 44.5 | 41.7! | 45.0 | 42.4! | 44.8 | 45.5 | 44.8 |
| `sixel_large` | 23.6 | 24.8 | 24.1 | 25.0 | 24.7 | 24.6 | 25.0 | 23.8 | 25.2 |
| `tui_redraw` | 332 | 319 | 320 | 310 | 302 | 381 | 371 | 325! | 340 |
| `unicode_symbols` | 227 | 229 | 226 | 224 | 229 | 214! | 352 | 362 | 361 |
| `vim_session` | 216 | 210 | 211 | 209 | 206 | 256 | 265 | 267 | 266 |

`!` marks a value below 0.95 x the column before it. Each was re-measured, interleaved with
the column before, best of 3 (or 4):

| Step | Fixture | Before -> after (best of N) | Verdict |
|---|---|---|---|
| S5 | `hyperlinks`, `osc_9_7`, `plain_ascii` | 279 -> 266, 253 -> 245, 666 -> 646 | within 5 % |
| S5 | `grapheme_clusters`, `light_cells`, `scrolling`, `unicode_symbols`, `combining_unique`, `sixel` | equal or faster | single-run noise |
| S6 | `scroll_region`, `tui_redraw`, `plain_ascii` | 360 -> 345, 381 -> 371, 658 -> 790 | within 5 % (with the block count; without it -7 %, -7 %, -4 %) |
| S7 | `tui_redraw`, `cursor_motion` | 378 -> 345, 308 -> 299 (plain build); 372 -> 381, 308 -> 308 (aligned build) | code placement, not work: see below |
| S7 | `mode_churn` | 212 -> 209 (plain), 220 -> 219 (aligned) | within 5 % |
| S8 | `long_lines`, `short_lines`, `light_cells` | 1,026 -> 1,046, 60.5 -> 59.1, 1,771 -> 1,794 | within 5 % |
| S2, S3, S6 | `plain_ascii`, `scrolling`, `scroll_region_small`, `sixel` | the step's code is not on the fixture's path; the next column is back | single-run noise (`sixel` spreads up to 15 % within a run) |

Code placement: S7 does not touch the `tui_redraw` path (no scroll there), yet the plain build
lost 9 %. Rebuilt with every function and branch target aligned
(`RUSTFLAGS="-C llvm-args=-align-all-functions=6 -C llvm-args=-align-all-nofallthru-blocks=5"`,
scratch only), S6 -> S7 is `tui_redraw` +2 %, `cursor_motion` 0 %, `scroll_region` +65 %.
Under the sampling profiler the two plain builds also differ by under 2 % on `tui_redraw`.
files.md turns this into the rule for guard fixtures.

## Rejected experiments (pinned MiB/s)

| Experiment | Result |
|---|---|
| tight restamp loop (verify pass, then stores only) | `scroll_region` 358 -> 361 |
| build empty window slots before rotating (S7 variant) | `scroll_region` 474 -> 397, `scroll_region_small` 33.7 -> 28.2 |
| reset without reading the last cell | `short_lines` 61 -> 62 (the first-cell miss remains) |
| no scroll events at all | `short_lines` +4 %, `scroll_region_small` +18 % (not allowed: events are API) |
| skip the anchor trim per line | `short_lines` +3 % |
| drop the `DIRTY` byte store in `RowMut::new` / whole-header reset | 0 % |
| conditional hint store | 0 % |
| single-byte print path | +3-4 % `cursor_motion`, `short_lines` |
| packed two-word `Style` hash | `dense_cells` -10 %, `heavy_sgr` -3 % |
| 16-byte ESC look-ahead | 0 % |
| `str::from_utf8` before `utf8_chunks` | 0 % |
| `set_cluster` for mark attach | `combining_unique` +4 % (and needs the `occ` rule) |
| BMP width table (2 bits per scalar) | `cjk_wide` +9 %, `unicode_symbols` +4 %: README "Later" |

## Prototype correctness

- `cargo test -p oneterm-vt --all-features` on the prototype: all pass except
  `slice_rotation_never_revives_a_stale_row` (planted state, tests.md) and the two
  `user_guide` tests that need the docs tree (absent in the scratch copy).
- A copy of the compare adapter built against the prototype (after S4, and after S8):
  `verify` dumps (4 checkpoints, final hash, scrollback rows) byte-identical to HEAD's adapter
  on all 23 corpus fixtures (HEAD is 21/21 ok, 2 unverified).

## What stays behind (README "Later")

| Fixture | After (proto) | Best | What is left (measured) |
|---|---:|---:|---|
| `short_lines` | 58-61 | 130 (rio) | ~17 % cold recycled rows; the rest spread: 1-letter print ~35 %, LF bookkeeping ~30 %; no single fix >= 5 % found |
| `cjk_wide`, `unicode_symbols` | 373, 361 | 403, 418 (ghostty) | decode + validate 25 %, width lookup 9-14 % (BMP table: +9 % / +4 %, needs a lazily built 16 KiB static) |
| `dense_cells`, `tui_redraw` | 240, 340-381 | 329, 484 (rio) | one derived-`Hash` style lookup per SGR; per-sequence dispatch; first-cell cache miss after a CUP |
| `scroll_region` | 578 | 764 (ghostty) | the 48-byte slot rotation itself; smaller slot entries would be the next structural step |
| `combining_unique` | 47 | 152 (vt100, no golden) | one random hash-table access per unique cluster and sweeps of ~1.6 M cells; vt100 keeps clusters inline |
