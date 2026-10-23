# Model: types, signatures, ladder (`input.rs`, `input/key.rs`, `input/mouse.rs`)

Notation in every table of this spec: cells are Rust byte-string bodies (`\x1b[1;5A`); `CSI` =
`\x1b[`, `SS3` = `\x1bO`, `ESC` = `\x1b`; `m` = modifier parameter (below).

## Key types (`input/key.rs`)

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyMods { pub shift: bool, pub ctrl: bool, pub alt: bool }

#[derive(Debug, Clone, Copy, PartialEq, Eq)] #[non_exhaustive]
pub enum NamedKey { Enter, Backspace, Delete, Tab, Escape, ArrowUp, ArrowDown, ArrowLeft,
    ArrowRight, Home, End, PageUp, PageDown, Insert, F1, F2, .., F24 }   // 38 variants, this order

#[derive(Debug, Clone, PartialEq, Eq)] #[non_exhaustive]
pub enum KeySpec { Character(String), Named(NamedKey) }   // Character: the text the key produced

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] #[non_exhaustive]
pub enum KeyEventKind { #[default] Press, Repeat, Release }

#[derive(Debug, Clone, PartialEq, Eq)] #[non_exhaustive]
pub struct KeyEvent {
    pub key: KeySpec, pub mods: KeyMods, pub kind: KeyEventKind,
    pub shifted: Option<char>,      // kitty REPORT_ALTERNATE_KEYS; sent only with shift held
    pub base_layout: Option<char>,  // same flag: the PC-101 key at this physical position
    pub text: Option<String>,       // REPORT_ASSOCIATED_TEXT; None -> Character payload (kitty.md)
}
impl KeyEvent { pub fn new(key: KeySpec, mods: KeyMods) -> KeyEvent; } // Press, the rest None

pub fn encode_key(key: &KeySpec, mods: KeyMods, modes: &ModeSnapshot) -> Option<Vec<u8>>;
pub fn encode_key_event(event: &KeyEvent, modes: &ModeSnapshot) -> Option<Vec<u8>>;
```

- `encode_key(k, m, s)` == `encode_key_event(&KeyEvent::new(k.clone(), m), s)`.
- `None`: the event sends nothing (unrequested release, no code point, no encoding such as
  Ctrl+`é`); the embedder drops it. `Some(bytes)` is never empty (D3).
- Rustdoc on `KeyEvent` and `encode_key_event` carries doc examples
  (`Escape` release builder; `CSI > 1 u` turns Escape into `\x1b[27u`).

## Mouse types (`input/mouse.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalMouseButton { Left, Middle, Right }          // codes 0, 1, 2
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseModifiers { pub shift: bool, pub alt: bool, pub ctrl: bool }  // +4, +8, +16

pub fn encode_mouse_press(row: usize, col: usize, button: TerminalMouseButton,
    modes: ModeSnapshot, mods: MouseModifiers) -> Vec<u8>;
pub fn encode_mouse_release(row: usize, col: usize, button: TerminalMouseButton,
    modes: ModeSnapshot, mods: MouseModifiers) -> Vec<u8>;
pub fn encode_mouse_move(row: usize, col: usize, button: Option<TerminalMouseButton>,
    modes: ModeSnapshot, mods: MouseModifiers) -> Vec<u8>;     // None = hover
pub fn encode_wheel_event(row: usize, col: usize, delta_y: f64,
    modes: ModeSnapshot, mods: MouseModifiers) -> Vec<u8>;     // > 0 up, < 0 down
```

`row`, `col`: zero-based viewport cell (from `Terminal::hit_test`). Empty `Vec` = write nothing.

## `Terminal` (`terminal.rs`)

```rust
impl Terminal {
    pub fn encode_key(&self, key: &KeySpec, mods: KeyMods) -> Option<Vec<u8>>;   // input::encode_key(.., &self.mode_snapshot())
    pub fn encode_key_event(&self, event: &KeyEvent) -> Option<Vec<u8>>;        // same for events
}
```

No mouse methods on `Terminal`: the embedder passes `term.mode_snapshot()`.

## `ModeSnapshot` fields read (spec 0008 palette-modes.md)

| Encoder | Reads | Never reads |
|---|---|---|
| kitty rungs (kitty.md) | `keyboard_flags` | |
| `modifyOtherKeys` rung | `modify_other_keys` (`0..=2`; a hand-built `3+` acts as `2`) | |
| legacy rung | `app_cursor` | `app_keypad`, LNM (Decision 4) |
| mouse encoders | `mouse` (`reporting`, `encoding`); `None` -> every encoder returns empty | |
| none | | `alt_screen`, `bracketed_paste`, `insert`, `show_cursor`, `alternate_scroll`, `reverse_video` |

A test flips every unread field and asserts identical bytes (tests.md `only_app_cursor_is_read`).

## The ladder (`encode_key_event`)

| Rung | Applies when | Result |
|---|---|---|
| 1 | `kind == Release` and the release is not reportable (kitty.md) | `None` |
| 2 | a kitty flag applies to this event (kitty.md) | kitty form, or `None` for an empty `Character` |
| 3 | `modify_other_keys != 0`, Ctrl or Alt held, key is an "other" key, level-1 exception not hit | `CSI 27;m;k~` |
| 4 | otherwise | legacy table (legacy.md); `Repeat` encodes as `Press` |

Rung 2 before 3: a program that pushed kitty flags and set `modifyOtherKeys` gets kitty. Once
rung 2 applies, `app_cursor` is not read (`\x1b[A`, never `\x1bOA`).

## Modifier parameter

`m = 1 + shift + 2*alt + 4*ctrl` (`1..=8`); shared by legacy, `modifyOtherKeys` and kitty
(crate `fn modifier_byte(KeyMods) -> u8`). Mouse uses its own mask (mouse.md).

## Internal shape (crate-private)

`kitty::encode(&KeyEvent, &ModeSnapshot) -> Encoded` with `enum Encoded { Silent, Bytes(Vec<u8>),
Legacy }`; `key::encode_legacy(&KeySpec, KeyMods, &ModeSnapshot) -> Option<Vec<u8>>`;
`kitty::unshifted(&str) -> Option<u32>` shared by rungs 2 and 3. Names may change; the bytes may not.
