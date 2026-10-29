# Acceptance: steps S1-S8

Pinned `vt-bench grid --fixture NAME --mib 32` (CPU 4, high priority), MiB/s, one reading per
line of the pinned procedure in files.md (spread 1-20 % on the noisy fixtures). The compare run
(S9) is not part of this file yet.

| Step | Commit subject | Fixture: before -> after (threshold) |
|---|---|---|
| S1 | `fix(intern): reclaim unused style ids ...` | `heavy_sgr` 262 -> 265, `dense_cells` 189 -> 188; compare verify `csi_mix` ok |
| S2 | `perf(grid): write printable ASCII runs in bulk ...` | `plain_ascii` 157 -> 446 (330), `long_lines` 162 -> 540 (440), `scrolling` 164 -> 595 (460), `light_cells` 175 -> 672 (540) |
| S3 | `perf(parser): scan printable ASCII in 16-byte blocks ...` | `plain_ascii` 446 -> 638 (500), `long_lines` 540 -> 836 (680), `scrolling` 595 -> 936 (650), `light_cells` 672 -> 1,274 (1,000); `tui_redraw` 357 -> 310 (see deviations) |
| S4 | `perf(grid): place wide glyphs in the print run ...` | `cjk_wide` 127 -> 228 (185), `unicode_symbols` 138 -> 223 (190), `tui_redraw` 314 -> 319 (260) |
| S5 | `perf(events): keep 32768 events between feeds ...` | `short_lines` 37 -> 62 (48) |
| S6 | `perf(grid): rotate region rows as one slice ...` | `scroll_region` 140 -> 300 (220), `scroll_region_small` 11 -> 28 (21) |
| S7 | `perf(intern): one intern per mark run, half-cap sweep ...` | `combining_unique` 1.4 -> 18 (14) |
| S8 | `perf(terminal): bulk ASCII under ?2027 ...` | `grapheme_clusters` 23 -> 59 (45) |

Other fixtures of the list stayed within noise at every step (`heavy_sgr` 250-270, `dense_cells`
188-194, `sixel` 41-44, `hyperlinks` 252-281).

## Gate and checks at the last commit

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace`, `cargo test --workspace --all-features`,
  `cargo test --workspace --no-default-features`: green (947 unit tests in `oneterm-vt`).
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`: green.
- `scripts/public-api.sh check`: unchanged.
- `vt-bench grid --check` (unpinned): PASS, 14 of 14 fixtures.

## Compare verify (oneterm adapter, release)

All 21 golden fixtures `ok` (`csi_mix` included); `combining_unique` and `unicode_symbols`
`unverified` (no golden), as expected.

## Deviations from the spec text

- `InternTable::sweep_unreferenced` drops freed values with one `HashMap::retain` instead of a
  `remove` per value (S1). With `remove` reachable for the style table, `dense_cells` fell 12 %
  (the shared probe stopped being inlined into the SGR path).
- `Row::recycle` carries `#[inline]` (S5): without it `scrolling` lost 11 %.
- `rotate` (S6) builds a never-written slot in its single restamping pass instead of a separate
  materialise pass; a separate pass reached only 14 MiB/s on `scroll_region_small`.
- `GraphemeRemap::kept()` counts its entries instead of reading a stored count, so the type stays
  a one-field tuple struct and the public-API snapshot does not change.
- `tui_redraw` lost 11 % at S3 (357 -> 310; 8-byte text runs between cursor moves pay the extra
  block scan); it is 1.6x its value before S2 and regained by nothing else. The 0.95 x rule is
  missed for that one fixture at that step.
- The half-cap of the sweep counts clusters without id 0 (what the S7 test `sweep_keeps_half_newest_first`
  needs); the amortisation test therefore asserts the trigger at the cap, not exactly `kept` entries.
- After review: `rotate` (S6) builds a slot as blank when it holds a row of another id, as
  `materialise` and `row` do (`scroll_region` 355, `scroll_region_small` 26 pinned); a separate
  `fix(grid)` commit makes `set_scrollback_limit` skip rows outside `oldest..=newest`.
