# 0011: Graphics / Sixel (phase 10)

**Goal:** `DCS q` Sixel decoded to RGBA (`GraphicData`), placed at the cursor as an anchored
`Placement` whose cells share one interned `Extras.graphic`, released with
`VtEvent::GraphicReleased` when its rows are gone; `SnapshotPlacement` filled; extras-table
reclamation; tier-2 and tier-3 `sixel` live.

**Non-goals:** other image protocols (kitty graphics, OSC 1337); DECSDM `?80`, P2 background
fill, pixel aspect, XTSMGRAPHICS, `CSI 16 t`; drawing, scaling, GPU upload; hyperlink reclamation.

| File | Covers |
|---|---|
| [sixel.md](sixel.md) | decoder: routing, grammar, colour registers, raster, painting, caps, `finish`, hostile input |
| [placement.md](placement.md) | public types, `Terminal` API, cell size, `place`, stamping, cursor rule, release causes, `take_graphics`, caps |
| [reclamation.md](reclamation.md) | extras free list, live-id walk, when the sweep runs, integrity |
| [snapshot.md](snapshot.md) | `SnapshotPlacement` filling, `graphic`, `graphic_offset`, damage rules, renderer recipe |
| [tests.md](tests.md) | decoder, lifecycle, routing, snapshot tests; bench; `docs/user/graphics.md` outline |
| [files.md](files.md) | files, edits, `lib.rs`, docs, rules, gate, commit |

**Public API** (`docs/roadmap.md` phase 10 list, nothing dropped): root `GraphicData, Placement, VIRTUAL_CELL`
(`SnapshotPlacement` exists since phase 7); `Terminal::{take_graphics, placements,
set_cell_pixels}`; `VtEvent::GraphicReleased(GraphicId)`; plus `FeedStats::graphics_dropped` if D2 is A.

## Acceptance criteria

- [ ] Quality gate; `cargo test -p oneterm-vt` with `--no-default-features` and `--features vt-paranoid`; `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features`
- [ ] Decoder caps hold on every hostile row of sixel.md: no panic, pixels <= 64 MiB, queue <= 128 MiB
- [ ] Placements scroll, region-scroll, IL/DL and reflow with their rows; every release cause in placement.md fires `GraphicReleased` exactly once
- [ ] 10,000 images in one session keep the extras table bounded, and no released id is recycled while a cell names it
- [ ] `vt-bench grid|render|all` include `sixel` (all 10 fixtures); numbers in tests.md acceptance notes
- [ ] `docs/user/graphics.md` exists, row 06 ready; events/limits/conformance chapters gain their rows
- [ ] Commit: `feat(graphics): Sixel decoder, image placements and graphic lifetime` (minor)

## Decisions (settled from DEC STD 070, xterm ctlseqs, specs 0002-0008)

1. Decoder subset: P1/P2/P3 ignored, untouched pixels transparent, the declared raster size wins (crops band padding), colours baked at paint time, register = `Pr mod 256`, default VT340 palette.
2. Footprint `ceil(px / cell)` against `set_cell_pixels`, `VIRTUAL_CELL` (10x20) when either axis is 0; footprints never recomputed.
3. Cursor: DEC/conhost rule, walk `band_pixels / cell_h` rows through the LF path, column kept; not xterm's.
4. Liveness from rows (anchor alive and an extent row with `HAS_GRAPHIC`); sweep at end of feed and in `resize`; releases delivered by the next `feed`, an empty one too.
5. Ids from 1, never reused (RIS keeps the counter); 256 live placements, oldest evicted; `take_graphics` is the only drain.
6. One decoder at a time; abort discards; the ESC of a following sequence finishes the image.
7. Internal: at most 5 decoder arguments held; extras sweep before interning, every 4,096 misses, skipping unhinted rows; integrity checks only "no cell names a freed extras id".

## Design choices with alternatives (reviewer rules A = as proposed, B = the simpler alternative)

| # | Proposed (A) | Simpler alternative (B) | Evidence |
|---|---|---|---|
| D1 | HLS hue reduced first: `(h % 360 + 240) % 360` | `(h + 240) % 360` | arguments saturate at `u32::MAX` (sixel.md), so `#0;1;4294967295;0;0` overflows: a debug panic on hostile input (roadmap rule) |
| D2 | queue capped at 128 MiB: oldest pending dropped, its placement released, `FeedStats::graphics_dropped` | unbounded queue until `take_graphics` | `"1;1;4096;4096` (about 20 bytes with `DCS q` and ST) decodes to 64 MiB (the declared raster wins, Decision 1), so a legal sequence reaches the pixel cap; roadmap: memory bounded, every drop counted |
| D3 | a newer image whose extent contains an older placement releases it at once | keep the older live while any of its rows has `HAS_GRAPHIC`, which the newer image sets again | row liveness (Decision 4) has this false positive; an animation redrawn at one spot keeps 256 dead images (the live cap) and their textures; renderers draw from placements and unpainted pixels are transparent (Decision 1), so stale frames show through |
| D4 | `CSI 14 t` reports the effective cell (`VIRTUAL_CELL` when unset): `4;480;800` at 24x80 | raw `cell_pixels`: `4;0;0` until set | the footprint cell (Decision 2) is the one `CSI 14 t` should report; xterm ctlseqs: `CSI 14 t` = text area size in pixels; 0007 D14 left these bytes to phase 10 |
| D5 | a placements change is a meta change: `Partial`, never `Unchanged` | refresh placements, never compare them | a release with no viewport row change (eviction, trim, D2/D3) would return `Unchanged`, so a retained renderer keeps showing an image whose texture it just freed |

Rows are independent: B on a row reverts only that row's rules and tests.

## Open items

None. Every candidate question (P2, `?80`, aspect, register range, raster vs data, cursor rule,
cell fallback, RIS, alt swap, trim, reflow, event delivery, extras reuse) was settled from
DEC STD 070, xterm ctlseqs or specs 0002-0008; D1-D5 need a ruling.
