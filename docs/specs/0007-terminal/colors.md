# Colours: `terminal/color.rs`

The engine keeps only the **OSC override layer**. The theme underneath, and resolving a cell's
`Color` to pixels, belong to the embedder (phase 7 adds `Palette` and `set_theme_colors`).

## `ColorKey`

```rust
#[non_exhaustive] #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ColorKey { Palette(u8), Foreground, Background, Cursor, SelectionBackground, SelectionForeground }
impl ColorKey {
    pub const fn index(self) -> usize;                    // slot in the flat table (below)
    pub const fn from_index(index: usize) -> Option<ColorKey>;   // None outside 0..COLOR_COUNT
    pub fn query_prefix(self) -> String;                  // the number a query reply echoes
    pub fn color_reply(self, color: Rgb, terminator: StringTerm) -> Vec<u8>;  // wraps query_prefix
}
```

| Key | `index()` | OSC set / query | OSC reset | `query_prefix()` |
|---|---|---|---|---|
| `Palette(n)` | `n` | `4;n;spec` | `104;n`, bare `104` (all 256) | `4;n` |
| `Foreground` | 256 | `10` | `110` | `10` |
| `Background` | 257 | `11` | `111` | `11` |
| `Cursor` | 258 | `12` | `112` | `12` |
| `SelectionBackground` | 269 | `17` | RIS only | `17` |
| `SelectionForeground` | 270 | `19` | RIS only | `19` |

`COLOR_COUNT = 271` (crate const). **Deliberate contract change vs the study material:** its
`BrightForeground`, `DimForeground` and `Dim(u8)` keys (no sequence writes them) are removed; the
indices are the study's (259..=268 is now a gap: `from_index` gives `None` there, its slots stay
`None`), so `index`/`from_index` values are a stable contract; `ColorKey` is `#[non_exhaustive]` so a
future key is a minor bump.

**`color_reply`** (an OSC colour-query reply is API, docs/user/versioning.md):
`ESC ] <query_prefix> ; rgb:<rr><rr>/<gg><gg>/<bb><bb> <T>`, each channel two lower-case hex
digits written twice (8 -> 16 bit, xterm's form), `<T>` = `BEL` (`07`) for `StringTerm::Bel`,
`ESC \` for `St`. Examples: `Background`, `{30,30,30}`, `Bel` -> `\x1b]11;rgb:1e1e/1e1e/1e1e\x07`;
`Palette(1)`, `{205,0,0}`, `St` -> `\x1b]4;1;rgb:cdcd/0000/0000\x1b\`.

## `ColorOverrides`

```rust
#[derive(Clone, Debug)] pub struct ColorOverrides { slots: Box<[Option<Rgb>; COLOR_COUNT]> }
impl Default for ColorOverrides { .. }                         // all None
pub fn get(&self, key: ColorKey) -> Option<Rgb>;               // None = the theme owns it
pub fn iter(&self) -> impl Iterator<Item = (usize, Rgb)> + '_; // set slots by index, ascending
pub(crate) fn set(&mut self, ColorKey, Rgb) -> bool;           // returns "changed" (phase 7 bumps epochs on it)
pub(crate) fn reset(&mut self, ColorKey) -> bool;
pub(crate) fn reset_palette(&mut self) -> bool;                // 0..=255 only (bare OSC 104)
pub(crate) fn reset_all(&mut self) -> bool;                    // RIS
```

`Terminal::color(key)` = `colors.get(key)`; `Terminal::colors()` = `&colors`.

## Colour specs (`parse_color(&[u8]) -> Option<Rgb>`, crate)

| Form | Rule |
|---|---|
| `#` + 3, 6, 9 or 12 hex digits | three equal groups; each channel = the group's first two digits (`#f00` -> `f0`: one digit is the high nibble) |
| `rgb:R/G/B` | each 1-4 hex digits, scaled `255 * v / (16^len - 1)` (`rgb:f/8/0` -> `{255, 136, 0}`) |
| anything else (names, `rgbi:`, wrong digit counts, extra `/`) | `None` |

Hex is case-insensitive. Palette index = a non-empty decimal run that fits `u8` (`256` -> `None`).

## Sequences (from osc.md's table)

`args(1..)` = params 1.. each split on `;` (only the 16th parameter can hold `;`; iterate, no
`Vec`). "Value" handling for one `(key, spec)`: `parse_color` ok -> `set`; spec `?` -> push
`ColorQuery { key, terminator }`; else unhandled.

| Code | Rule |
|---|---|
| `4` | fewer than 2 args -> unhandled. Walk args as `(index, spec)` pairs; a bad index -> unhandled, pair skipped; a trailing odd arg is ignored, not counted |
| `10`, `11`, `12` | fewer than 1 arg -> unhandled. Each arg in turn with key `10`, `11`, `12` advancing from the code; an arg past `12` -> unhandled, stop |
| `17`, `19` | `p1` missing or empty -> unhandled; else one value with that key; any further param -> one more unhandled |
| `104` | no arg or empty first arg -> `reset_palette`; else each arg an index -> `reset(Palette)`, bad index -> unhandled |
| `110` / `111` / `112` | `reset(Foreground / Background / Cursor)` (params ignored) |

- Several queries in one OSC (`4;1;?;2;?`) push one `ColorQuery` each, in order.
- RIS: `reset_all`. DECSTR keeps overrides.
