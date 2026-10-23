# Files to touch

Builds on specs 0002-0008 as implemented (phases 6 and 7 land first). No new dependency
(`rustc-hash` and `std::sync::Arc` only). Sub-milestones, gate green after each:
**10a** extras reclamation (reclamation.md) and OSC 8 through `intern_extras`; **10b** decoder
and its tests; **10c** placement, lifetime, `Terminal` API, events, DCS routing, `CSI 14 t`;
**10d** snapshot filling, bench tiers, user chapters.

## `crates/vt/src` (new)

| Path | Content | Milestone |
|---|---|---|
| `graphics.rs` | module doc (three rules, format-agnostic RGBA); `mod sixel; mod placement;`; `VIRTUAL_CELL`, `GraphicData`, `Placement`, `GraphicsState` (+ `reset` for RIS), crate consts; `#[cfg(test)] mod graphics_tests;` | 10c |
| `graphics/sixel.rs` | `SixelDecoder`, `DecodedSixel`, default palette, `hls_to_rgb`, `percent` (sixel.md); `#[cfg(test)] #[path = "sixel_tests.rs"] mod sixel_tests;` | 10b |
| `graphics/placement.rs` | `place`, `stamp`, `effective_cell`, `cells_for`, `release`, `release_covered`, `sweep`, `drain_released`, `push_pending` (placement.md) | 10c |
| `graphics/sixel_tests.rs`, `graphics/graphics_tests.rs` | tests.md | 10b, 10c |

## `crates/vt/src` (edits)

| Path | Change |
|---|---|
| `intern.rs`, `intern_tests.rs` | free list, `since_sweep`, `needs_sweep`, `sweep_unreferenced`, `is_free`, `clear` rule (reclamation.md); 0003's "never move" module-doc line becomes "styles and hyperlinks never move; extras ids are reclaimed by a full-grid sweep" |
| `grid/terminal_grid.rs`, `grid/screen.rs` | `live_extras_ids` (+ a per-screen collector); tests in `terminal_grid_tests.rs` |
| `grid/row.rs` | `check_cell_ids`: a free extras id -> `UnknownId::Extras` |
| `terminal.rs` | `State.graphics: GraphicsState`; `intern_extras`; `take_graphics`, `placements`, `set_cell_pixels`; `feed`: empty input drains `released` before returning; after the grapheme sweep: placement sweep, drain, extras sweep if due; `resize` sweeps after the grid resize; the 0007 hook row for phase 10 is now done |
| `terminal/dispatch.rs` | `dcs_hook` / `dcs_put` / `dcs_unhook` Sixel arms (sixel.md); `place` gets the handler so the walk uses the LF path |
| `terminal/query.rs` | `CSI 14 t` from the effective cell (**D4**) |
| `terminal/modes.rs` | RIS: `graphics.reset()` |
| `terminal/osc.rs` | OSC 8 pen update through `intern_extras` |
| `events.rs` | `VtEvent::GraphicReleased(GraphicId)`; `FeedStats::graphics_dropped: u32` (**D2**) |
| `snapshot/state.rs`, `snapshot/row.rs` | `EngineView.placements`, refill + meta compare (**D5**), `scratch`; rustdoc "until phase 10" notes removed |
| `lib.rs` | below |

```rust
mod graphics;
pub use graphics::{GraphicData, Placement, VIRTUAL_CELL};

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/graphics.md")]
mod user_guide_graphics {}
```

The crate doc gains: "Sixel images are decoded to RGBA, anchored to the cells they cover and
released with an event when those cells are gone."

## `crates/vt-bench/src`

| Path | Change |
|---|---|
| `throughput.rs` | tier-2 filter removed (every tier runs all ten fixtures); `grid_pass` drains `take_graphics` per feed; tests renamed (tests.md) |
| `render.rs` | same filter removed, frame loop drains `take_graphics`; `render_runs_every_fixture` |

## Docs

| Path | Change |
|---|---|
| `docs/user/graphics.md` | new chapter (tests.md outline); row 06 of `docs/user/README.md` -> `ready` with a link |
| `docs/user/events.md` | `GraphicReleased` row; `graphics_dropped` counter |
| `docs/user/limits.md` | rows: 4096 px per axis (pixels past it dropped), 64 MiB decode buffer, 128 MiB queue (`graphics_dropped`), 256 placements (oldest released), extras sweep every 4,096 new entries |
| `docs/user/conformance.md` | Sixel supported (scrolling mode, DEC/conhost cursor rule); gaps: P2, aspect, `?80`, XTSMGRAPHICS, `CSI 16 t` |
| `docs/specs/0007-terminal/queries.md` | `CSI 14 t` row: effective cell (D4 outcome) |
| this spec's tests.md | bench acceptance notes |

## Rules

- No `unsafe`; no `unwrap`/`expect`/`panic!`/`unreachable!` outside tests; every `as` on a value
  already bounded (`MAX_DIMENSION`, `& 0xFF`); `u16` footprints via `try_from` + saturate.
- `#![warn(missing_docs)]`: `GraphicData` states RGBA8 straight alpha and stride; `Placement`
  says it is read-only; `take_graphics` says it is the only drain and the per-cycle order;
  `set_cell_pixels` names `VIRTUAL_CELL` and `CSI 14 t`; rustdoc names no repo paths.
- `Terminal` stays `Send + Sync` (existing test); no lock held anywhere; no `log` calls.
- No `#[allow]`; `#[cfg(test)]` accessors only where tests.md says so.

## Gate (beyond the quality gate)

```bash
cargo test -p oneterm-vt --no-default-features
cargo test -p oneterm-vt --features vt-paranoid
RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features
```

## Commit

`feat(graphics): Sixel decoder, image placements and graphic lifetime` (minor; body names
`Spec: docs/specs/0011-graphics` and the rulings on D1-D5).
