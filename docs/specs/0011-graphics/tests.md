# Tests, bench, user chapter

Harness (graphics_tests.rs): `Session { term, batch, now }` with `feed`, `one_image()` (exactly one
from `take_graphics`), `graphic(row_index, col)`, `offset(row_index, col) -> Option<(GraphicId,
u16, u16)>` (via a `SnapshotState`), `cursor()`, `released()` (ids from the last batch),
`replies()`; `sixel(body)` wraps `ESC P q .. ESC \`. Default cell unless a test sets one.

## Decoder (`graphics/sixel_tests.rs`, on `SixelDecoder` directly)

| Test | Assert |
|---|---|
| `minimal_image` | `#0;2;100;0;0#0~` -> 1x6, every pixel `[255, 0, 0, 255]` |
| `rgb_and_hls_registers` | `#0;2;100;0;0#1;2;0;0;100#0~$#1?~` -> 2x6, (0,0) red, (1,0) blue; HLS hue 120 red, 0 blue, 240 green |
| `repeat_and_bands` | `!3A` -> 3x6, only row 1 set; `!0~` paints once; `!5$` drops the repeat; `~-~` -> 1x12 |
| `untouched_pixels_are_transparent` | `DCS 0;1;0 q` body `#0!3A`: (2,1) red, (2,0) and (0,0) `[0, 0, 0, 0]` |
| `raster_wins` | `"1;1;1;3` + two bands -> 1x3; absent -> measured 1x12; larger raster pads transparent; 3 args or a 0 axis ignored |
| `default_palette_and_register_wrap` | register 2 = `[204, 33, 33, 255]`; `#257;2;100;0;0#1~` paints red; `#1;7;1;2;3#1~` keeps register 1 |
| `colour_baked_at_paint_time` | paint with 0, redefine 0, paint: the first column keeps the old colour |
| `blank_sixel_extends_size` | `?` alone -> 1x6 transparent; empty payload and `#0` alone -> `None` |
| `band_pixels_counts_bands_above_the_last` | `~-~` -> 6; `~-~-` -> 12 |
| one test per hostile row of sixel.md | as listed there (`hls_hue_max_does_not_overflow` is the **D1** regression) |

## Placement and lifetime (`graphics/graphics_tests.rs`, through `Terminal`)

| Group | Tests (one behaviour each) |
|---|---|
| geometry | `placed_at_cursor_column_clipped_right` (25x45 at (2,3) -> 3x3; 80 px wide at col 2 of 4 -> `cols == 2`, cell text untouched); `cursor_walk_is_band_pixels_over_cell_height` (1, 4, 5, 8 bands at 60 px -> rows down 0, 0, 1, 2); `footprint_follows_cell_pixels` (384x576: 9x18 -> 43x32, 18x36 -> 22x16, unset -> 39x29; cursor on the last row); `one_pixel_image_is_one_cell`; `zero_axis_falls_back` ((9,0), (0,18), (0,0)); `footprint_is_ceil` (8 size/cell rows); `cursor_keeps_column`; `taller_than_screen_scrolls_and_stays_anchored`; `wider_than_screen_clipped_not_wrapped`; `height_not_multiple_of_six`; `raster_larger_than_data_cursor_inside`; `raster_smaller_than_data_cursor_past`; `cell_change_keeps_earlier_footprint`; `one_pixel_cell_does_not_hang`; `trailing_graphics_newline_moves_below` |
| walk side effects | `walk_counts_lines_and_scroll_events` (`lines_produced`, `rows_scrolled`, `RowsScrolled` match the LFs); `walk_honours_scroll_region` |
| cells | `one_extras_entry_per_image` (4000x4000 on 400x210: entries +1, footprint 400x200); `two_images_stay_distinct`; `text_over_a_cell_drops_the_reference`; `ed2_primary_keeps_image_in_history`; `merged_link_and_image_keep_both` |
| movement | `scrolls_into_history_with_its_cells`; `moves_with_in_region_scroll` (SU then IL); `reflow_keeps_anchor_on_its_row` (grow and shrink width, anchor on the same content) |
| release | `released_on_alt_clear` (exactly once); `released_on_ris`; `released_on_blanking_region_scroll`; `released_when_anchor_trimmed`; `released_when_reflow_drops_anchor` (event on the next feed); `released_on_alt_wipe` (`?1049 l`, `?1049 h`); `release_delivered_by_empty_feed`; `released_before_repaint`; `eviction_at_256` (257 one-cell images -> 256 live, oldest released) |
| D3 | `newer_image_covering_older_releases_it` (same cell, two feeds: second batch holds the first id, one placement); `smaller_newer_image_keeps_older`; `cover_ignores_other_lane` |
| no recycling | `evicted_screen_cell_not_recycled_into_link`; `evicted_history_cell_not_recycled`; `swept_row_not_recycled` (IL split + region scroll); `evicted_merged_entry_does_not_split_link`; `every_live_placement_reachable_by_its_cells`; `trimmed_anchor_survivors_not_recycled` (4,200 links after) |
| hand-off | `image_handed_out_once`; `ris_drops_pending_keeps_id_counter`; `clear_screen_keeps_pending`; `images_survive_suppressed_frame` (`?2026`, two states); `aborted_dcs_stamps_no_cells`; `payload_cap_aborts_endless_sixel` (32 MiB); `corrupt_band_does_not_panic` |
| D2 | `pending_cap_drops_oldest` (three raster-only 4096x4096 images, no take: `graphics_dropped == 1`, first id released, two taken; the one memory-heavy test) |
| D4 / DA1 | `csi_14_t_reports_effective_cell` (24x80 unset -> `ESC [ 4 ; 480 ; 800 t`; 9x18 -> `4;432;720`; (9,0) -> fallback); `da1_advertises_sixel` (`ESC [ ? 6 2 ; 4 ; 2 2 c`) |
| reclamation | `ten_thousand_images_keep_extras_bounded` (5-row screen, scrollback 0, image + CRLF x 10,000: `entries() <= 4_200`, the last image's cell names its id); `freed_extras_id_fails_integrity` (`check_integrity` -> `UnknownId::Extras` after freeing a held id, `#[cfg(test)]` access) |

## DCS routing (same file, own section)

`intermediate_dcs_q_never_decodes` (`$ q m`, `+ q 544e`: no decoder, unhandled 0, nothing placed,
row 0 blank); `bare_dcs_q_places_one`; `parameterised_dcs_q_decodes` (`DCS 0;1;0 q`, `DCS 9 q`,
unhandled 0); `query_after_empty_unterminated_sixel` (no decoder after, nothing placed);
`query_after_nonempty_unterminated_sixel` (first image placed once, `aborted_dcs 0`);
`one_mib_query_payload_buffers_nothing` (unhandled 1, no decoder, nothing pending);
`overflowed_intermediates_not_sixel` (`DCS $ + ! q ~~~ ST`: unhandled 1). No 8-bit `ST` test:
our parser never ends a string on `0x9C` (0002 D4).

## Elsewhere

| File | Tests |
|---|---|
| `intern_tests.rs` | `freed_slot_reused_first`; `sweep_frees_exactly_unreferenced`; `every_miss_counts_towards_sweep` (full table too); `clear_empties_free_list`; `id_zero_never_freed` |
| `terminal_grid_tests.rs` | `live_extras_ids_reads_history_and_templates` (history cell, both pens, both saved cursors); `live_extras_ids_skips_unhinted_rows` |
| `snapshot_tests.rs` | `placement_refills` (pixel_size, col, `graphic_offset` over 3x3, outside -> `None`, unknown id -> `None`, cells carry the id); `dead_anchor_skipped`; `stale_graphic_has_no_placement`; `placement_only_change_is_partial` (**D5**, eviction while scrolled back); `suppressed_frame_holds_placement_meta`; `placement_refresh_allocates_nothing` (capacities flat over 100 updates) |
| events (0007 per-counter rule) | `graphics_dropped_counts_per_feed` (shares the D2 fixture) |

## Bench

- Tier 2 and tier 3 run all ten fixtures: the `sixel` filter of 0007 bench.md and 0008 bench.md
  is removed (no filter parameter is left), `tier2_skips_sixel` / `render_skips_sixel` become
  `tier2_runs_every_fixture` / `render_runs_every_fixture` (10 rows, table order).
- `grid_pass` and the render frame loop call `term.take_graphics()` after every `feed`, inside
  the timed region (an embedder drains each feed; for other fixtures it is an empty `mem::take`).
- **Acceptance notes:** machine, `cargo run -p oneterm-vt-bench --release -- all --mib 32`, and
  the `sixel` rows of tiers 2 and 3 are pasted here by the coder (target 47 MiB/s is phase 12's).

## `docs/user/graphics.md` (chapter 06, <= 120 lines, Rust blocks are doctests)

1. Example: feed a one-pixel Sixel, `take_graphics`, `snapshot_update`, iterate `placements()`,
   match `VtEvent::GraphicReleased` (asserts on `rgba.len()`).
2. Lifecycle diagram: `DCS q` -> `GraphicData` -> placement -> `SnapshotPlacement` -> release.
3. Embedder checklist: per-cycle order (placement.md); `set_cell_pixels` on every font or DPI
   change; key textures by `GraphicId`; free on `GraphicReleased`; ids never repeat.
4. Geometry: footprint, `VIRTUAL_CELL`, the cursor rule, raster vs data, `CSI 14 t`.
5. Drawing: by cells with `graphic_offset` (recommended) or by placement; 1:1 pixels, clip.
6. Ceilings: 4096 px per axis, 16 MiB payload, 256 placements, 128 MiB queue (`graphics_dropped`).
7. Not supported: P2 background, pixel aspect, `?80`, XTSMGRAPHICS, other image protocols.
