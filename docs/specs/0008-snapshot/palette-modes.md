# `Palette` (`snapshot/palette.rs`) and `ModeSnapshot` (`snapshot/mode_snapshot.rs`)

## `Palette`

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Palette {
    pub indexed: [Rgb; 256],                // 16 ANSI, 6x6x6 cube, 24 greys
    pub foreground: Rgb, pub background: Rgb, pub cursor: Rgb,
    pub bright_foreground: Option<Rgb>,     // None -> foreground
    pub dim_foreground: Option<Rgb>,        // None -> dim(foreground)
}
impl Palette {
    pub fn new() -> Palette;                // defaults below
    pub fn resolve(&self, color: Color) -> Rgb;
    pub fn dim(&self, color: Rgb) -> Rgb;   // 50 % mix with `background`, rounding half up
}
impl Default for Palette { fn default() -> Palette { Palette::new() } }
```

| `new()` slot | Value |
|---|---|
| `indexed[0..16]` | `000000 cd0000 00cd00 cdcd00 0000ee cd00cd 00cdcd e5e5e5 7f7f7f ff0000 00ff00 ffff00 5c5cff ff00ff 00ffff ffffff` |
| `indexed[16 + 36r + 6g + b]` | each channel from `[00, 5f, 87, af, d7, ff]` |
| `indexed[232 + i]` | grey `8 + 10 i` (`080808` .. `eeeeee`) |
| `foreground`, `cursor` / `background` | `d8d8d8` / `181818` |
| `bright_foreground`, `dim_foreground` | `None` |

`resolve`: `Rgb(c)` -> `c`; `Palette(i)` -> `indexed[i]`; `Named`: `Black..=White` ->
`indexed[0..8]`, `BrightBlack..=BrightWhite` -> `indexed[8..16]`, `Foreground` / `Background` /
`Cursor` -> the field, `BrightForeground` -> `bright_foreground.unwrap_or(foreground)`,
`DimForeground` -> `dim_foreground.unwrap_or(dim(foreground))`, `DimBlack..=DimWhite` ->
`dim(indexed[0..8])`. `dim`: per channel `(c + bg).div_ceil(2)` in `u16`, so `{205,0,0}` over
black -> `{103,0,0}`, over `{40,40,60}` -> `{123,20,30}`.

## Theme vs OSC overrides

| Layer | Owner | Set by | Read by |
|---|---|---|---|
| theme | embedder | `Terminal::set_theme_colors(&mut self, theme: &Palette)`, stored as `State.theme` | `palette()` (D4) |
| overrides | the stream | OSC 4/10/11/12/17/19 and resets (spec 0007 colors.md) | `Terminal::color`, `colors()` |
| effective | | | `map_colors(&palette)` |

Precedence, per slot: an override wins; resetting it (OSC 104/110/111/112, RIS) returns the
slot to the theme. `SelectionBackground`/`SelectionForeground` overrides have no `Palette` slot:
the embedder draws selections and reads them through `Terminal::color`.

**D4** `Terminal::palette(&self) -> Palette`: `State.theme` with `indexed[i] =
colors.get(ColorKey::Palette(i))`, `foreground`/`background`/`cursor` = the `Foreground`/
`Background`/`Cursor` overrides, each only where set; `bright_`/`dim_foreground` from the theme.
Copying ~800 bytes is cheap under the lock; the embedder then calls `state.map_colors(&palette)`
outside it. If D4 is ruled B, the embedder applies `colors().iter()` itself (same precedence,
documented in `docs/user/snapshot.md`) and `State.theme` stays write-only.

`set_theme_colors`: `State.theme = *theme`; bumps `palette_epoch` only when it differs (**D2**),
so the next update of every state is `Full` and remaps every row. `Terminal::new` starts with
`Palette::new()` as the theme.

## `ModeSnapshot`

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MouseReporting { X10, Normal, ButtonEvent, AnyEvent }          // ?9, ?1000, ?1002, ?1003
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum MouseEncoding { #[default] Default, Utf8, Sgr, Urxvt }         // -, ?1005, ?1006, ?1015
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MouseProtocol { pub reporting: MouseReporting, pub encoding: MouseEncoding }
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)] #[non_exhaustive]
pub struct ModeSnapshot { /* pub fields below */ }
impl Default for ModeSnapshot { .. }    // power-on: show_cursor and alternate_scroll true, rest off
```

| Field | Type | `Terminal::mode_snapshot()` reads |
|---|---|---|
| `alt_screen` | `bool` | `grid.alt_active()` |
| `app_cursor`, `app_keypad` | `bool` | `Mode::AppCursor` (DECCKM), `Mode::AppKeypad` (DECKPAM) bits |
| `bracketed_paste`, `insert` | `bool` | `?2004`, `4` (IRM) bits |
| `show_cursor` | `bool` | `?25` (DECTCEM) bit |
| `alternate_scroll` | `bool` | `?1007` bit |
| `reverse_video` | `bool` | `?5` (DECSCNM): the embedder swaps default fg/bg; no cell changes |
| `mouse` | `Option<MouseProtocol>` | `Modes::mouse_protocol()` (below) |
| `keyboard_flags` | `KeyboardFlags` | `keyboard.active.live` (kitty) |
| `modify_other_keys` | `u8` | `modify_other_keys` (`0..=2`) |

- `#[non_exhaustive]`: a new field is a minor bump; outside the crate start from `default()` or
  `mode_snapshot()` and assign fields.
- `ModeSnapshot::default() == Terminal::new(..).mode_snapshot()` (test) with the default config.
- `Terminal::mode_snapshot(&self) -> ModeSnapshot` and `Terminal::mouse_reporting(&self) ->
  Option<MouseProtocol>` (= `mode_snapshot().mouse` without building the rest).

## Mouse derivation: crate `Modes::mouse_protocol(&self) -> Option<MouseProtocol>`

| Step | Rule |
|---|---|
| reporting | first set bit of `?1003` -> `AnyEvent`, `?1002` -> `ButtonEvent`, `?1000` -> `Normal`, `?9` -> `X10`; none -> return `None` |
| encoding | first set bit of `?1006` -> `Sgr`, `?1015` -> `Urxvt`, `?1005` -> `Utf8`; none -> `Default` |

The setters already keep each group to one bit (spec 0007 modes.md: `h` clears the other members
first, `l` clears only itself), so the priority order only guards hand-built bit sets. An
encoding bit without a reporting bit gives `None`; it applies once reporting is turned on.
