# Files to touch

Builds on the phase-3 grid (spec 0004) as implemented; names below match its code
(`Row::rehome`, the push helper `push_row`, `RowRef::allocated` / `blank`).

## `crates/vt/src`

| Action | Path | Content |
|---|---|---|
| create | `grid/screen/reflow.rs` | `impl Screen`: `reflow_columns`, `clip_columns`, `host_cursor_row`; `Point`, `PointOwner`, `LineCells`, `RowSpan`, `lay_out`; columns.md, cursor-anchors.md, algorithm.md |
| create | `grid/screen/resize.rs` | `impl Screen`: `resize_rows`, `shift_cursor_to`, release of bottom rows, `restate_viewport`; rows.md |
| create | `grid/screen/reflow_tests.rs` | `#[path]` child of `reflow.rs` (tests.md) |
| create | `grid/resize_tests.rs`, `grid/resize_props.rs` | `#[path]` children of `terminal_grid.rs` (tests.md) |
| edit | `grid/screen.rs` | `mod reflow; mod resize;`; field `newest_issued` (init `newest`); push helper rule (rows.md); `TabStops::resize`; `Screen::heap_bytes` unchanged |
| edit | `grid/screen/integrity.rs` | tier 1: `newest <= newest_issued` -> `IdsOutOfOrder` |
| edit | `grid/row.rs` | crate `fit`, `into_cells`, `from_cells` (columns.md, algorithm.md) + tests in `row_tests.rs` |
| edit | `grid/anchor.rs` | crate `kill(id)`; `AnchorKind::is_screen_owned` -> `pub(crate)` + test in `anchor_tests.rs` |
| edit | `grid/terminal_grid.rs` | `ResizePolicy`, `ResizeOutcome` (rustdoc per cursor-anchors.md); field `reflow_rows`; `pub fn resize`; `heap_bytes` adds the scratch; test module decls |
| edit | `grid/mod.rs` | `mod terminal_grid` -> `pub(crate) mod terminal_grid` (root re-export path); module doc: ids-rewind exception (rows.md); `grid::*` list unchanged |
| edit | `grid/grid_fixture.rs` | `resize`, `lines`, `cursor_at`, `mark` (tests.md) |
| edit | `lib.rs` | below |

`lib.rs` (additive; the module doc line on `grid` gains "resize and reflow"):

```rust
pub use grid::terminal_grid::{ResizeOutcome, ResizePolicy};

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/resize.md")]
mod user_guide_resize {}
```

The types are `pub` items in a `pub(crate)` module, so they are public only at the root (the
roadmap list), not as `grid::ResizePolicy`.

## Rules for this code

- Every `pub` item has rustdoc (`#![warn(missing_docs)]`) naming units, clamps and what dies;
  rustdoc names no repo paths or spec files.
- No `unwrap`/`expect`/`panic!`/`unreachable!`; slice access by `get` or after a clamp with a short
  comment. Offsets inside a line are `usize`; narrowing to `u16` only after a clamp to `cols`.
- No allocation per output row except a needed row buffer (algorithm.md); no per-line `Vec`, no
  `collect`, no `VecDeque`. Counters saturate.
- The new `pub(crate)` items all have a non-test caller in this phase (`resize`), so the grid's
  `cfg_attr(not(test), expect(dead_code, ..))` keeps covering only the phase-6 ops. If a new item
  is reported unused, the attribute must not be widened: call it or delete it.
- No `log`, no `#[inline]`, no `hotpath` attributes.

## Dependencies

None new. `proptest` (dev) since phase 1.

## Docs

| Action | Path |
|---|---|
| create | `docs/user/resize.md` (coder; outline in tests.md) |
| edit | `docs/user/grid.md` (coder): the id rule gains the resize exception (rows.md) |
| done by architect | `docs/user/README.md` row 07, `docs/README.md` spec row |

## Commit

No CHANGELOG (release notes come from Conventional Commits at tag time). Subject (minor):
`feat(reflow): column reflow, row resize and ResizePolicy for TerminalGrid`
