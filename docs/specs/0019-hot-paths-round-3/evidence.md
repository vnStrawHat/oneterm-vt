# Evidence: profiles, prototype, re-checks

Machine i7-12700, Windows 11, rustc 1.96.0, release profile (thin LTO, line tables). "Pinned"
= one process on logical CPU 4 at high priority (`start /affinity 10 /high`), 160x45, 10,000
history lines, 64 KiB chunks, `default-features = false`, 1 warm-up + 5 passes, median. Corpus:
`.tmp/corpus/*.vt` (32 MiB each, the compare corpus v1). HEAD = `1fb640c`.

## Tools (scratch, `.tmp/s19`, not part of the spec)

- Runner and sampling profiler from 0018 (evidence.md there); the profiler now names samples
  in system DLLs (`ntdll` by nearest export, `VCRUNTIME140`) from the process's module list.
- Other engines: the compare adapters' own `grid` command, pinned (`--scrollback N`).
- Prototype: branch `steps2` of the scratch repo, one commit per step S1-S6 from HEAD (plus
  the test edits of tests.md); `steps` holds S5's first form, `master` the discovery order.

## Where the time goes at HEAD (pinned profiles)

| Fixture | Finding | Step |
|---|---|---|
| `long_osc` | per-byte OSC table inlined in `feed`: 94 % | S1 |
| `hyperlinks` | per-byte OSC table 69 %; after S1: `Utf8Chunks` 19 %, heap 22 % (boxed lookup keys) | S1, S2 |
| `osc_9_7` | per-byte OSC table 29 %; after S1: lossy decode 16 %, `text_span` re-validation | S1, S2 |
| `sixel_large` | `ntdll` 32 % (large blocks, faults), `memset`/`memcpy` 16 %, `finish` copy loop 20 % | S3 (S1) |
| `sixel` | `collect_extras_ids` 15 %; `release_covered` ~16 %; `dcs_put` + parser 14 % | S4, S1 (rest left) |
| `short_lines` | rio 16 ns per hot line, we 40 ns (scrolling.md); region-scroll machinery and an anchor walk per LF | S5 |
| `scroll_region`, `scroll_region_small` | `memmove` of 48-byte slots 17 % / 14 % | S6 |
| `dense_cells`, `tui_redraw` | SGR path ~40 % (lookup 20 %); per-sequence call chain | (left) |
| `combining_unique` | `GraphemeArena::intern` 27 %, index insert 18 %, one per unique cluster | (left) |

## Prototype per step (pinned MiB/s, cumulative, spec order, one run each)

| Fixture | HEAD | S1 | S2 | S3 | S4 | S5 | S6 |
|---|---:|---:|---:|---:|---:|---:|---:|
| `cjk_wide` | 372 | 351! | 333! | 360 | 324! | 372 | 337! |
| `combining_unique` | 45.2 | 47.6 | 42.1! | 43.9 | 43.1 | 42.3 | 46.5 |
| `csi_mix` | 172 | 167 | 166 | 166 | 166 | 162 | 170 |
| `cursor_motion` | 304 | 299 | 288 | 291 | 277 | 268 | 301 |
| `dense_cells` | 239 | 238 | 235 | 230 | 219 | 226 | 241 |
| `grapheme_clusters` | 57.0 | 58.2 | 56.8 | 52.6! | 55.2 | 57.7 | 57.1 |
| `heavy_sgr` | 300 | 303 | 296 | 291 | 304 | 301 | 300 |
| `hyperlinks` | 278 | 867 | 1220 | 1213 | 1216 | 1125! | 1245 |
| `light_cells` | 1773 | 1800 | 1797 | 1683! | 1710 | 1672 | 1824 |
| `long_lines` | 1040 | 1034 | 1010 | 992 | 1043 | 814! | 1067 |
| `long_osc` | 904 | 9931 | 9261! | 8679! | 9430 | 9382 | 9313 |
| `mode_churn` | 210 | 209 | 208 | 204 | 200 | 209 | 209 |
| `osc_9_7` | 238 | 286 | 344 | 319! | 321 | 354 | 369 |
| `plain_ascii` | 745 | 754 | 735 | 707 | 690 | 815 | 806 |
| `scroll_region` | 562 | 556 | 543 | 529 | 565 | 563 | 608 |
| `scroll_region_small` | 59.7 | 59.1 | 54.9! | 53.0 | 55.7 | 60.8 | 65.1 |
| `scrolling` | 1165 | 898! | 1143 | 948! | 1087 | 1245 | 1173! |
| `short_lines` | 57.3 | 54.2! | 57.9 | 57.3 | 57.3 | 64.6 | 63.3 |
| `sixel` | 44.9 | 40.7! | 45.2 | 41.3! | 64.9 | 71.6 | 71.3 |
| `sixel_large` | 25.1 | 20.7! | 25.7 | 42.0 | 50.3 | 52.0 | 52.0 |
| `tui_redraw` | 344 | 316! | 358 | 339! | 362 | 361 | 340! |
| `unicode_symbols` | 361 | 294! | 350 | 361 | 362 | 356 | 358 |
| `vim_session` | 266 | 258 | 256 | 256 | 260 | 266 | 267 |

`!` marks a value below 0.95 x the column before it; every one was re-measured below.

## Re-checks (interleaved, best of 3; before / after the step)

| Step | Fixture: before / after | Verdict |
|---|---|---|
| S1 | `cjk_wide` 372 / 372, `unicode_symbols` 361 / 358, `scrolling` 1,168 / 1,183, `short_lines` 58.4 / 57.9, `sixel` 45.1 / 47.3, `sixel_large` 26.1 / 26.6, `tui_redraw` 343 / 355 | single-run noise |
| S2 | `cjk_wide` 369 / 354 (0.96), `combining_unique` 47.0 / 47.5, `long_osc` 9,732 / 9,234 (0.949), `scroll_region_small` 57.7 / 54.3 (0.941) | S2 is on none of these paths (no OSC 8/9 text, OSC 6 is dropped): aligned builds below |
| S3 | `cjk_wide` 358 / 357, `grapheme_clusters` 56.4 / 56.3, `light_cells` 1,714 / 1,756, `long_osc` 9,478 / 9,385, `osc_9_7` 321 / 331, `scrolling` 1,047 / 1,198, `sixel` 46.1 / 54.0, `tui_redraw` 350 / 346 | noise; `sixel` +17 % from S3 |
| S4 | `cjk_wide` 346 / 356, `cursor_motion` 285 / 286, `dense_cells` 231 / 231 | noise |
| S5 | `long_lines` 1,035 / 1,055, `hyperlinks` 1,246 / 1,251, `combining_unique` 47.3 / 47.1 (first form: `cursor_motion` 286 / 290, `grapheme_clusters` 57.3 / 57.7, `long_osc` 9,161 / 9,633) | noise |
| S6 | `scroll_region` 558 / 616, `scroll_region_small` 59.8 / 64.3, `cjk_wide` 375 / 369, `short_lines` 65.2 / 64.6, `sixel` 72.4 / 74.1, `plain_ascii` 827 / 828, `tui_redraw` 358 / 347, `scrolling` 1,274 / 1,175 (0.92; best of 5: 1,238 / 1,164, 0.94) | `scrolling` is on S6's path (a pointer per row access): aligned builds below |

Aligned builds (`-align-all-functions=6 -align-all-nofallthru-blocks=5`, scratch only) and
the profiler for the two open rows:

| Step | Fixture | Plain builds | Aligned builds | Profiler (samples, before / after) | Verdict |
|---|---|---|---|---|---|
| S2 | `scroll_region_small` | 57.3 / 54.3; each of the three S2 changes alone 56.7-57.3 | 58.8 / 54.5 | 30,506 / 29,982; no function above +3 % | placement: S2's code is not on the path, only the three changes together move it |
| S2 | `long_osc`, `cjk_wide` | 9,732 / 9,234, 369 / 354 | 9,500 / 9,408, 373 / 370 | | placement |
| S6 | `scrolling` | 1,238 / 1,164 (best of 5) | 1,278 / 1,226 (0.96) | | a real 4-6 % cost: README, owner decision |
| S6 | `short_lines`, `tui_redraw`, `scroll_region` | 65.2 / 64.6, 358 / 347, 558 / 616 | 65.2 / 63.2, 364 / 377, 594 / 678 | | `short_lines` -1 to -3 %; gains hold aligned |
| S6 (first S5 form) | `cjk_wide`, `plain_ascii`, `unicode_symbols` | 369 / 346, 797 / 785, 348 / 349 | 361 / 361, 771 / 823, 339 / 356 | | placement |
The machine drifts up to 10 % between blocks: compare within a row only.

## Rejected experiments (pinned MiB/s, interleaved best of 3 unless noted)

| Experiment | Result |
|---|---|
| pen style memo, 1,024 direct-mapped slots verified against the table (rio's `StyleSet` memo) | `dense_cells` 237 -> 257 (+8 %), `heavy_sgr` 303 -> 295, `tui_redraw` 373 -> 362 |
| one `str::from_utf8` per ground run, then `print_text` | `short_lines` +4 %, `scroll_region` +6 %, `plain_ascii` +4 %; `tui_redraw` -4 %, `cjk_wide` -5 % |
| 16-byte `ESC` look-ahead (again) | `tui_redraw` 375 -> 372, `cursor_motion` 303 -> 297 |
| LF fast path without skipping anchor walks | `short_lines` 57.9 -> 61.3, `osc_9_7` 331 -> 353 (with the skip: 66.7, 367; S5 keeps both) |
| S5 skipping walks by a count of non-screen-owned anchors (first form) | same on `short_lines`; `sixel` no gain (256 placements keep the count above 0, `Screen::linefeed` 15 % of `sixel`): replaced by the watermark |
| DCS run as its own step after S3/S4 | `sixel` 59.5 -> 64.7, `sixel_large` 48.9 -> 52.3: folded into S1 (same technique, same loop change) |

## Prototype correctness

- `cargo test -p oneterm-vt --all-features` on `steps2` with tests.md's edits: 1,072 pass;
  fails only the anchor test tests.md changes and the two `user_guide` tests that need the
  docs tree (absent in the scratch copy). `cargo clippy --all-targets --all-features -D
  warnings`: clean.
- A copy of the compare adapter built against the prototype (after S6, both S5 forms):
  `verify` dumps (4 checkpoints, final hash, scrollback rows) byte-identical to HEAD's
  adapter on all 23 corpus fixtures (HEAD is 21/21 ok, 2 unverified).
- Images (the dumps hold cells only): every image of `sixel`, `sixel_large` and 3,000 random
  Sixel strings (raster before, after or without paint; paint past the raster; repeats;
  `CAN` aborts) hashed through `take_graphics` (size and RGBA): identical to HEAD
  (479,350, 596 and 2,691 images).

## What stays behind

| Fixture | After S5 (proto) | Best | What is left (measured) |
|---|---:|---:|---|
| `short_lines` | 65 | 128 (rio) | a 1-byte print still walks parser run -> `print_str` -> `print_scalars` -> `print_run` -> `row_mut` (~24 ns hot, rio's whole line is 16 ns); the cold recycled row (~8 ns, rio ~6 ns); one `RowsTrimmed` per line (+4 % without any scroll event, 0018) |
| `osc_9_7` | 360 | 495 (rio) | the same per-line costs as `short_lines`, plus building the `Notification` (joined into `scratch`, then copied into the event arena) |
| `tui_redraw`, `dense_cells` | 361, 226-241 | 470, 316 (rio) | per-sequence dispatch and the SGR path (~40 % of `dense_cells`); the memo above was the only lever found and gave +8 % |
| `sixel` | 72 | 79 (rio) | `release_covered` reads every live placement's anchor per new image (most of `dcs_unhook` 28 %, 256 at the cap); `paint` 18 %; heap 16 %; `dcs_put` 13 % |
| `scroll_region_small` | 60 (64 with S6) | 79 (ghostty) | an alternate-screen partial region still goes through `scroll_up` (`clamp_scroll`, report, `shift_region`, now skipped by the watermark) and the slot rotation (S6) |
| `scroll_region` | 565 (616 with S6) | 753 (ghostty) | the 48-byte slot rotation (S6, owner decision); `print_run` 33 % (47 letters per line) |
| `long_osc` | 8,700-9,900 | 13,320 (ghostty) | the 2 KiB copy per unrouted OSC that limits.md promises; 16-byte blocks (SSE2) for the scan |
| `combining_unique` | 47 | 150 (vt100, no golden) | one random index insert per unique cluster (vt100 keeps clusters inline, no dedup) |
