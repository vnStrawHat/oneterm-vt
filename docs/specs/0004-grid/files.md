# Files to touch

## `crates/vt/src`

| Action | Path | Content |
|---|---|---|
| create | `grid/mod.rs` | module doc (ids are positions, screen vs viewport, anchors); dead-code attribute (below); `mod` decls; `pub use` of the API list; `RowId`, `Pos`, `Size`, `Viewport`, `ScrollRegion`, `MAX_ROWS`, `MAX_COLS`, `SCROLLBACK_MAX`, `DEFAULT_SCROLLBACK` ([terminal-grid.md](terminal-grid.md)) |
| create | `grid/row.rs` | [rows.md](rows.md) + `Row::check` |
| create | `grid/screen.rs` | [screen.md](screen.md): types, `Screen`, ring, viewport, cursor, region, tabs, scroll primitives, `sync_anchors`, `heap_bytes`, `row_text`; `mod print; mod erase; mod integrity;` |
| create | `grid/screen/print.rs` | [screen-edit.md](screen-edit.md): `PrintMode`, `print`, `wrapline`, `put_tab`, `dropped_wide` |
| create | `grid/screen/erase.rs` | [screen-edit.md](screen-edit.md): `LineClear`, `DisplayClear`, EL/ECH/DCH/ICH/ED |
| create | `grid/screen/integrity.rs` | tier 1-2 checks ([paranoid.md](paranoid.md)) |
| create | `grid/integrity.rs` | `IntegrityViolation` |
| create | `grid/anchor.rs` | [anchors.md](anchors.md) |
| create | `grid/terminal_grid.rs` | `TerminalGrid`, tier 3 check |
| create | `grid/grid_fixture.rs`, `grid/row_tests.rs`, `grid/screen/screen_tests.rs`, `grid/anchor_tests.rs`, `grid/terminal_grid_tests.rs`, `grid/grid_props.rs` | [tests.md](tests.md) |
| edit | `cell.rs` | add `pub(crate) fn is_erasable(self, interner: &Interner) -> bool` ([screen-edit.md](screen-edit.md)) + 3 cases in `cell_tests.rs` (blank, `'\t'`, inverse/graphic not erasable) |
| edit | `lib.rs` | below |

`grid` public items (exactly): `Anchor, AnchorId, AnchorKind, Anchors, Charset, Cursor,
CursorOrigin, DEFAULT_SCROLLBACK, DisplayClear, LineClear, Pos, PrintMode, Row, RowFlags,
RowHeader, RowId, RowMut, RowRef, RowsScrolled, SCROLLBACK_MAX, Screen, ScreenKind,
ScrollRegion, ScrollReport, SeqNo, Size, TabStops, TerminalGrid, Viewport`.

`lib.rs` (additive; the module doc gains one line naming `grid`):

```rust
pub mod grid;

pub use grid::{Pos, RowId, RowRef, SeqNo, Size, Viewport};

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/grid.md")]
mod user_guide_grid {}
```

## Dead-code attribute (README open item 1)

The crate-private VT ops (`print`, `linefeed`, erase, IL/DL, `enter_alt`, ...) get their non-test
caller in phase 6. Until then, at the top of `grid/mod.rs`:

```rust
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "crate-private grid operations are dispatched by Terminal from phase 6")
)]
```

When phase 6 calls them, the expectation goes unfulfilled, `-D warnings` fails, and the line is
deleted: it cannot outlive its reason. Test builds see every item used.

## Rules for this code

- Every `pub` item has rustdoc (crate is `#![warn(missing_docs)]`); rustdoc names no repo paths.
- No `unwrap`/`expect`/`panic!`/`unreachable!`; no slice index that input can push out of range
  (`get`/`get_mut`, or a range clamped first with a comment). `debug_assert!` only for the
  integrity results.
- Narrowing casts (`as u16`, `as u32`) only after a clamp that proves the fit, with a short
  comment. Counters saturate.
- No allocation in the scroll primitives beyond materialising a `None` row (no `Vec`, no `collect`).
- No `log`, no `#[inline]`, no `hotpath` attributes in this phase.
- Phase 6 items named in non-goals stay out (no reverse wrap, no `lines_produced`, no limit edit).

## Dependencies

None new: `bitflags` (`RowFlags`) is already a dependency since phase 2; `proptest` is a
dev-dependency since phase 1.

## Docs

| Action | Path |
|---|---|
| create | `docs/user/grid.md` (coder; outline in [tests.md](tests.md)) |
| done by architect | `docs/user/README.md` row 16, `docs/README.md` spec row |

## Commit

No CHANGELOG (release notes come from Conventional Commits at tag time). Subject (minor):
`feat(grid): ring-buffer screens with scrollback, anchors and stable row ids`
