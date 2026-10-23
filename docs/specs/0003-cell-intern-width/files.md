# Files to touch

## `crates/vt`

| Action | Path | Content |
|---|---|---|
| edit | `Cargo.toml` | `[dependencies]` add `bitflags`, `rustc-hash`, `unicode-segmentation`, `unicode-width`, each `.workspace = true` (versions already in the root `[workspace.dependencies]`) |
| edit | root `Cargo.toml` | `unicode-width = "=0.2.2"` (exact pin: the width table is a compatibility contract; any table change must be a deliberate bump) |
| edit | `src/lib.rs` | additive only (the parser lands in the same file): module doc names cells, styles and widths; `mod cell; pub mod intern; mod width;`; root re-exports; the chapter doctest module |
| create | `src/cell.rs` | [cell.md](cell.md): `Cell` + layout consts + size assert, `CellContent`, `CellWidth`, `Semantic`, `Attrs`, `Color`, `Rgb`, `NamedColor`, `Style` |
| create | `src/cell_tests.rs` | [tests.md](tests.md) `cell_tests.rs` |
| create | `src/intern.rs` | [intern.md](intern.md) + [graphemes.md](graphemes.md): ids, `Extras`, `InternTable`, `GraphemeArena`, `GraphemeRemap`, `Hyperlink`, `HyperlinkTable`, `Interner`, caps |
| create | `src/intern_tests.rs` | [tests.md](tests.md) `intern_tests.rs` |
| create | `src/width.rs` | [width.md](width.md): `scalar_width`, `cluster_width` |
| create | `src/width_tests.rs` | [tests.md](tests.md) `width_tests.rs` |

`lib.rs` additions:

```rust
mod cell;
pub mod intern;
mod width;

pub use cell::{Attrs, Cell, CellContent, CellWidth, Color, NamedColor, Rgb, Semantic, Style};
pub use width::{cluster_width, scalar_width};

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/cells-and-styles.md")]
mod user_guide_cells_and_styles {}
```

`intern` public items (exactly): `StyleId, ExtrasId, GraphemeId, HyperlinkId, GraphicId,
Extras, InternTable, GraphemeArena, GraphemeRemap, Hyperlink, HyperlinkTable, Interner,
GRAPHEME_SWEEP_ENTRIES, GRAPHEME_SWEEP_CHARS`. `GRAPHEME_ID_LIMIT` is `pub(crate)` (used by
`cell.rs`); every other cap is private.

## Rules for this code

- Every `pub` item has rustdoc stating units, caps and fallbacks (crate is
  `#![warn(missing_docs)]`); rustdoc names no repo paths or spec files.
- No `unwrap`/`expect`/`panic!`/`unreachable!` and no slice indexing that input can push out of
  range. The one allowed index is `entries[0]` in `InternTable::resolve`, with a comment that
  the constructor guarantees it.
- No `pub(crate)` item without a caller in this phase (`dead_code` fails `-D warnings`): grid
  predicates, wide-pair repair, `InternTable::clear` and extras sweep wait for their phases.
- Counters use `saturating_add`. Narrowing casts (`len as u16`, `as u8`) only after a cap
  check that proves the fit; say so in a short comment.
- No `log`, no `#[inline]`, no `hotpath` attributes in this phase.
- `cell.rs` and `intern.rs` reference each other (`Cell` uses ids, `InternTable<Style>` uses
  `Style`); that is one crate, not a dependency cycle, and stays as is.

## Docs

| Action | Path |
|---|---|
| create | `docs/user/cells-and-styles.md` (coder; outline in [tests.md](tests.md)) |
| done by architect | `docs/user/README.md` row 15, `docs/README.md` spec row |

## Commit

No CHANGELOG (release notes come from Conventional Commits at tag time). Commit subject:
`feat(cell): packed Cell, intern tables and Unicode widths` (minor).
