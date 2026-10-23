# Files to touch

Builds on the grid (spec 0004) and resize (spec 0005) as implemented. Phase 4 must land first:
the reflow tests call `TerminalGrid::resize`.

## `crates/vt/src`

| Action | Path | Content |
|---|---|---|
| create | `selection.rs` | module doc (model.md); `SelectionKind`, `Side`, `SelectionRange` (+ `contains`, `contains_cell`), `Selection`, `Invalidation`, `DEFAULT_SEMANTIC_ESCAPE_CHARS`; clamp, `Simple`/`Block` side rules, wide snap, `invalidated_by`; `mod expand; mod text;` + test module decls |
| create | `selection/expand.rs` | `next`/`prev`, word, bracket, line searches, `EXPAND_BUDGET` (expand.md); items `pub(super)` |
| create | `selection/text.rs` | `selection_text` and the per-row piece (text.md) |
| create | `selection/selection_tests.rs`, `selection/selection_props.rs` | tests.md (`#[cfg(test)] mod selection_tests; #[cfg(test)] mod selection_props;` in `selection.rs`) |
| edit | `grid/terminal_grid.rs` | `pub(crate) fn anchors(&self) -> &Anchors`, `pub(crate) fn anchors_mut(&mut self) -> &mut Anchors` |
| edit | `grid/screen.rs` | `pub(crate) fn tabs(&self) -> &TabStops` |
| edit | `grid/mod.rs` | `#[cfg(test)] mod grid_fixture;` -> `#[cfg(test)] pub(crate) mod grid_fixture;` |
| edit | `lib.rs` | below |

No grid item becomes `pub`; `grid::*` keeps its list. If the grid tests already reach the
anchors through the private field, switching them to the accessor is optional, not required.

`lib.rs` (additive; the crate doc gains one sentence naming selections):

```rust
mod selection;

pub use selection::{Invalidation, Selection, SelectionKind, SelectionRange, Side};

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/selection.md")]
mod user_guide_selection {}
```

`selection` is a private module: the five types are public only at the root (roadmap list).

## Rules for this code

- Every `pub` item has rustdoc (`#![warn(missing_docs)]`) stating inclusivity, clamping, what
  `None` means, and the `release` duty; rustdoc names no repo paths or spec files.
- No `unwrap`/`expect`/`panic!`/`unreachable!`; `RowRef::cell` already returns `Cell::EMPTY` out
  of range; `RowId` arithmetic saturates; column math on `u16` uses `saturating_*` or a clamp.
- `to_range`, `invalidated_by`, `contains*`: no allocation, no interner. `text`: one `String`.
- The new grid accessors have non-test callers here (`Selection`), so the grid's module-level
  `expect(dead_code, ..)` is neither widened nor needed for them. The only new expectation is
  the item-level one on `DEFAULT_SEMANTIC_ESCAPE_CHARS` (expand.md); phase 6 deletes it.
- No `log`, no `#[inline]`, no `hotpath` attributes.

## Dependencies

None new. `proptest` (dev) since phase 1.

## Docs

| Action | Path |
|---|---|
| create | `docs/user/selection.md` (coder; outline in tests.md) |
| done by architect | `docs/user/README.md` row 17, `docs/README.md` spec row |

## Commit

No CHANGELOG (release notes come from Conventional Commits at tag time). Subject (minor):
`feat(selection): anchored selections with word, line and block modes`
