# `cell.rs`: `Cell`, `Style`, `Attrs`, colours

Module doc: a `Cell` is 8 bytes with no pointer; everything rare (style, link, image, long
cluster) is an id into the terminal's `intern::Interner`, so uniform text stores one style.

## `Cell` bit layout (`#[repr(transparent)] pub struct Cell(u64)`)

| Bits | Field | Values |
|---|---|---|
| 0..21 | content | scalar value, or a `GraphemeId` when bit 21 is set |
| 21 | is_grapheme | 0 scalar, 1 grapheme id |
| 22..24 | width | 0 `Narrow`, 1 `Wide`, 2 `WideSpacer`, 3 `LeadingWideSpacer` |
| 24..26 | semantic | 0 `None`, 1 `Prompt`, 2 `Input`, 3 `Output` |
| 26 | protected | DECSCA; stored, honoured by selective erase in phase 6 |
| 27..32 | reserved | always 0; no setter can set them |
| 32..48 | style id | `StyleId` (0 = `StyleId::DEFAULT`) |
| 48..64 | extras id | `ExtrasId` (0 = `ExtrasId::NONE`) |

- `const _: () = assert!(size_of::<Cell>() == 8);` next to the type.
- Private consts per field (`CONTENT_MASK`, `GRAPHEME_BIT`, `WIDTH_SHIFT`, ...); one
  `fn with_field(self, mask, shift, value)` private helper is fine; no bit math elsewhere.
- **Canonical content**: `' '` and `'\0'` are stored as content 0, and content 0 reads back as
  `' '`. So `Cell::EMPTY.with_content(Scalar(' ')) == Cell::EMPTY` (derived `Eq` is bitwise and
  a blank scalar has one encoding). The engine never prints NUL.
- **Grapheme id range**: an id `>= GRAPHEME_ID_LIMIT` (crate const in `intern.rs`, `1 << 18`,
  well inside 21 bits) is stored as `GraphemeId(0)`, which resolves to `" "`. Never panics.
- Derives: `Clone, Copy, PartialEq, Eq, Hash, Default` (default = `EMPTY`). Manual `Debug`
  printing `content, width, semantic, protected, style, extras`.

## `Cell` API (all `pub`; `#[must_use]` on every `with_*`)

```rust
pub const EMPTY: Cell;                         // bits 0: ' ', Narrow, None, unprotected, default style, no extras
pub fn content(self) -> CellContent;
pub fn width(self) -> CellWidth;
pub fn semantic(self) -> Semantic;
pub fn is_protected(self) -> bool;
pub fn style_id(self) -> StyleId;
pub fn extras_id(self) -> ExtrasId;
pub fn with_content(self, content: CellContent) -> Cell;   // canonicalises ' ' / '\0'; clamps ids
pub fn with_width(self, width: CellWidth) -> Cell;
pub fn with_semantic(self, semantic: Semantic) -> Cell;
pub fn with_protected(self, protected: bool) -> Cell;
pub fn with_style(self, id: StyleId) -> Cell;
pub fn with_extras(self, id: ExtrasId) -> Cell;
pub fn text_char(self, graphemes: &GraphemeArena) -> char; // scalar, or the cluster's first scalar; ' ' if empty
pub fn to_bits(self) -> u64;                   // raw word for tests/snapshots; layout not a semver promise
```

- Each `with_*` changes only its own bits (proptest in tests.md).
- `semantic()` / `is_protected()` are `pub` (the setters are; a write-only field is useless to
  an embedder walking cells for prompt jumps). README decision 7.
- Not in this phase (no caller yet, `dead_code` under `-D warnings`): blank/erasable
  predicates and wide-pair repair. They arrive with the grid (phase 3).

## `CellContent`, `CellWidth`, `Semantic`

```rust
pub enum CellContent { Scalar(char), Grapheme(GraphemeId) }      // Clone, Copy, Eq, Hash, Debug
pub enum CellWidth { #[default] Narrow, Wide, WideSpacer, LeadingWideSpacer } // + Default
impl CellWidth { pub const fn is_spacer(self) -> bool }          // WideSpacer | LeadingWideSpacer
pub enum Semantic { #[default] None, Prompt, Input, Output }     // OSC 133 A..B, B..C, C..D
```

`CellWidth` is one enum, not flags, so "wide glyph with no spacer role" is unrepresentable.
`LeadingWideSpacer` = blank in the last column standing in for a wide glyph that wrapped.
Bits-to-enum decoding: `match bits & 0b11`, the last value in the `_` arm (all 4 values are used).

## `Attrs` (`bitflags!`, `u16`; derives `Clone, Copy, PartialEq, Eq, Hash, Debug, Default`)

| Flag | Bit | SGR | | Flag | Bit | SGR |
|---|---|---|---|---|---|---|
| `BOLD` | 0 | 1 | | `UNDERLINE` | 8 | 4 / 4:1 |
| `DIM` | 1 | 2 | | `DOUBLE_UNDERLINE` | 9 | 21 / 4:2 |
| `ITALIC` | 2 | 3 | | `UNDERCURL` | 10 | 4:3 |
| `INVERSE` | 3 | 7 | | `DOTTED_UNDERLINE` | 11 | 4:4 |
| `HIDDEN` | 4 | 8 | | `DASHED_UNDERLINE` | 12 | 4:5 |
| `STRIKEOUT` | 5 | 9 | | `OVERLINE` | 13 | 53 |
| `BLINK_SLOW` | 6 | 5 | | | | |
| `BLINK_FAST` | 7 | 6 | | `ALL_UNDERLINES` | 8..=12 | union of the five underline flags |

Bits 14-15 unused. Which SGR sets which flag is phase 6; this phase only defines storage.

## Colours

```rust
pub enum Color { Named(NamedColor), Palette(u8), Rgb(Rgb) }  // Clone, Copy, PartialEq, Eq, Hash, Debug
pub struct Rgb { pub r: u8, pub g: u8, pub b: u8 }           // same derives
pub enum NamedColor { .. }                                    // same derives; exhaustive
```

`NamedColor`, 29 variants in this order (README decision 8):

| Group | Variants |
|---|---|
| palette 0-7 | `Black, Red, Green, Yellow, Blue, Magenta, Cyan, White` |
| palette 8-15 | `BrightBlack, BrightRed, BrightGreen, BrightYellow, BrightBlue, BrightMagenta, BrightCyan, BrightWhite` |
| special slots | `Foreground` (OSC 10), `Background` (OSC 11), `Cursor` (OSC 12), `BrightForeground`, `DimForeground` |
| dim 0-7 | `DimBlack, DimRed, DimGreen, DimYellow, DimBlue, DimMagenta, DimCyan, DimWhite` |

A `Color` is what the stream named; resolution to pixels (theme, OSC 4/10/11 overrides) is
the embedder's or phase 7's job.

## `Style`

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Style { pub fg: Color, pub bg: Color, pub underline_color: Option<Color>, pub attrs: Attrs }
impl Style { pub const DEFAULT: Style = Style { fg: Color::Named(NamedColor::Foreground),
    bg: Color::Named(NamedColor::Background), underline_color: None, attrs: Attrs::empty() }; }
impl Default for Style { fn default() -> Self { Style::DEFAULT } }
```

`underline_color` is SGR 58/59, stored as given. Test-only const assert:
`size_of::<Style>() <= 16` (it is a hash key in a hot table).
