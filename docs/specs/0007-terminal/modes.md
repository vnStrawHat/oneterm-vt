# Modes, cursor style, keyboard, resets: `terminal/modes.rs`

## `Mode`, `ModeState`

```rust
#[non_exhaustive] #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)] pub enum Mode { .. } // table
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)] #[repr(u8)]
pub enum ModeState { NotSupported = 0, Set = 1, Reset = 2, PermanentlySet = 3, PermanentlyReset = 4 }
impl Mode { pub const fn inert_state(self) -> Option<ModeState>; }  // fixed DECRQM answer, or None
```

Crate-only: `from_private(u16)`, `from_ansi(u16)`, `private_code()`, and test lists of every
private and ANSI mode. `Modes { bits: u32 }` (crate) stores one bit per stateful mode.

| Number | Variant | State lives in | Power-on | `h` / `l` side effect |
|---|---|---|---|---|
| `?1` | `AppCursor` | bit | reset | |
| `?3` | `DecCoLm` | nothing (inert: `0`) | | both: invalidate `EraseScreen`, full region, home, every screen row reset with the erase cell; width unchanged |
| `?5` | `ReverseVideo` | bit | reset | screen flag, no cell changes |
| `?6` | `Origin` | bit | reset | both: home (`goto_origin(0, 0, new origin)`) |
| `?7` | `LineWrap` | bit | **set** | |
| `?9` `?1000` `?1002` `?1003` | `MouseX10`, `MouseClick`, `MouseDrag`, `MouseMotion` | bits | reset | `h` clears the other three first; `l` clears only itself |
| `?12` | `CursorBlink` | `cursor_style.blinking` | off | `cursor_style = Some(current with blinking = h)` |
| `?25` | `ShowCursor` | bit | **set** | |
| `?45` | `ReverseWrap` | bit | reset | read by BS |
| `?47` `?1047` `?1049` | `AltScreen47`, `AltScreen1047`, `AltScreen` | `grid.alt_active()` | primary | below |
| `?1004` | `FocusInOut` | bit | reset | |
| `?1005` `?1006` `?1015` | `Utf8Mouse`, `SgrMouse`, `UrxvtMouse` | bits | reset | `h` clears the other two first |
| `?1007` | `AlternateScroll` | bit | **set** | |
| `?1042` | `UrgencyHints` | bit | **set** | |
| `?1048` | `SaveCursor1048` | nothing (answers `2`) | | `h` DECSC, `l` DECRC |
| `?2004` | `BracketedPaste` | bit | reset | |
| `?2026` | `SyncUpdate` | bit (phase 7: `SyncState`) | reset | |
| `?2027` | `GraphemeClusters` | bit | reset | `l` drops the carry |
| `?9001` | `Win32Input` | bit (inert: `2`) | reset | conhost sends `h` unprompted; accepted silently |
| `4` | `Insert` | bit | reset | |
| `20` | `LineFeedNewLine` | bit (inert: `2`) | reset | LF never implies CR |
| `ESC =` `ESC >` | `AppKeypad` | bit | reset | no DECRQM number |

- `inert_state`: `DecCoLm` -> `NotSupported`, `Win32Input` and `LineFeedNewLine` -> `Reset`, every
  other `None`. Rule: DECRQM never answers `Set` for a mode nothing reads.
- `Terminal::mode(m) -> bool`: alt-screen variants -> `alt_active`; `CursorBlink` -> blink of the
  stored-or-default style; `DecCoLm`, `SaveCursor1048` -> `false`; else the bit.
- DECRQM state (queries.md): unknown number -> `0`; `inert_state` if `Some`; alt variants ->
  `alt_active`; `CursorBlink` -> blink; `SaveCursor1048` -> `2`; else the bit (`1` / `2`).

## Alternate screens (spec 0004 composition, xterm semantics)

`enter(save)`: alt active -> nothing. Else invalidate `SwapAlt`; if `save`, DECSC (with shifts);
`grid.enter_alt()`; `keyboard.swap()`. `leave()`: alt inactive -> nothing. Else invalidate
`SwapAlt`; `grid.leave_alt()`; `keyboard.swap()`.

| Mode | `h` | `l` |
|---|---|---|
| `?47` | `enter(false)` | `leave()` |
| `?1047` | `enter(false)` | if alt active: `grid.wipe()`, `leave()` |
| `?1049` | if alt inactive: `enter(true)`, then `grid.wipe()` (uses the entering pen) | if alt active: `leave()`, then DECRC (with shifts) |

## Cursor style

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum CursorShape { #[default] Block, Underline, Beam, HollowBlock, Hidden }
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct CursorStyle { pub shape: CursorShape, pub blinking: bool }
```

- Stored `cursor_style: Option<CursorStyle>`; `None` = `config.default_cursor_style`.
- DECSCUSR `CSI Ps SP q`: `0` -> `None`; `1|2` Block, `3|4` Underline, `5|6` Beam, blinking = `Ps`
  odd; other `Ps` unhandled. OSC 50: osc.md.
- `Terminal::cursor_style()`: stored-or-default; with `ShowCursor` reset the shape is `Hidden`
  (blink kept). No sequence stores `Hidden` or `HollowBlock`; `HollowBlock` is an embedder-side
  value (e.g. an unfocused window, or `Config::default_cursor_style`).
- `CursorStyleChanged` is pushed after DECSCUSR, OSC 50, `?12`, `?25`, DECSTR and RIS whenever
  `cursor_style()` differs from its value before that sequence.

## Kitty keyboard flags and `modifyOtherKeys`

```rust
bitflags! { #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)] pub struct KeyboardFlags: u8 {
    const DISAMBIGUATE_ESC_CODES = 1; const REPORT_EVENT_TYPES = 2; const REPORT_ALTERNATE_KEYS = 4;
    const REPORT_ALL_KEYS_AS_ESC = 8; const REPORT_ASSOCIATED_TEXT = 16; } }
```

- Crate `FlagStack { flags: [KeyboardFlags; 8], len: u8, live: KeyboardFlags }`; crate
  `KeyboardStacks { active, inactive }`, swapped on every real screen switch, default on RIS.
- `CSI > f u`: push `f & 0x1F`; a full stack (8) drops its oldest; `live = f`.
- `CSI < n u` (`n = next_or(1)`): `n >= len` -> empty; else `len -= n`; `live` = new top or empty.
- `CSI = f ; m u`: `live` := `f` (m 1, absent, other), `live | f` (m 2), `live & !f` (m 3); stack
  untouched. `CSI ? u`: reply with `live` (queries.md). `Terminal::keyboard_flags()` = `live`.
- `CSI > 4 ; v m` (`v = next_or(0)`): `v` in `0..=2` -> `modify_other_keys = v`; other `v`, or a
  first param other than `4` (default 1), -> unhandled. `CSI ? 4 m` replies; other `? m` unhandled.

## Resets

**RIS** (`ESC c`): invalidate `Reset`; `grid.reset()`; `interner.hyperlinks.clear()` and
`interner.extras.clear()` (no cell or pen holds an extras id after the grid reset); `modes`,
`keyboard`, `colors`, `title` to default; `cursor_style = None`; `modify_other_keys = 0`; shifts
to `{0, None, [(0, None); 2]}`; `preceding_char`, carry and DCS query cleared; push
`ScreenCleared`; `CursorStyleChanged` rule. Kept: row ids, `lines_produced`, `config`, the
style table and the grapheme arena.

**DECSTR** (`CSI ! p`): `Origin`, `Insert` reset; `LineWrap`, `ShowCursor` set; full region;
template := `Cell::EMPTY`; `goto(0, 0)`; DCS query cleared; DECSC (with shifts);
`CursorStyleChanged` rule. Kept: screen, title, colours, designations, shifts, cursor style,
keyboard, `AppCursor`, mouse modes.
