# Rung 4: legacy (xterm) encoding (`input/key.rs::encode_legacy`)

Reached when no kitty flag applies and `modifyOtherKeys` did not fire (model.md). Reads only
`app_cursor`. A `Repeat` encodes as a `Press`. `m` = `1 + shift + 2*alt + 4*ctrl`.

## `KeySpec::Character(s)`

| Held | Bytes |
|---|---|
| `s` empty | `None` (**D3**; B: `Some([])`, with Alt `Some([0x1b])`) |
| neither Ctrl nor Alt (Shift ignored: the layout already applied it) | `s` as UTF-8, whole |
| Ctrl | control table below; `None` if `s` is not exactly one ASCII scalar |
| Alt | `\x1b` + the bytes of the row above (Ctrl+Alt+`a` = `\x1b\x01`, Ctrl+Alt+`[` = `\x1b\x1b`) |

### Control table (Ctrl + one ASCII scalar; case-insensitive for letters)

| Scalar | Byte | Scalar | Byte |
|---|---|---|---|
| `a`..`z`, `A`..`Z` | `byte & 0x1f` (`c` -> `0x03`, `A` -> `0x01`) | `6` `^` `~` | `0x1e` |
| ` ` `2` `@` | `0x00` | `7` `_` `/` | `0x1f` |
| `3` `[` | `0x1b` | `8` `?` | `0x7f` |
| `4` `\` | `0x1c` | any other ASCII (`0`, `1`, `9`, `,`, `.`, `;`, ...) | the scalar itself |
| `5` `]` | `0x1d` | non-ASCII (`é`) or 2+ scalars (`ab`) | `None` |

Source: kitty spec "Legacy ctrl mapping of ASCII keys" (it lists `~` -> 30 and `/` -> 31).

## Named keys

Columns: no modifier / any modifier in the stated set. "blind" = modifiers do not change the bytes.

| Key | Plain | With modifiers |
|---|---|---|
| `Enter` | `\r` | Shift or Ctrl: `\x1b[13;{m}u` (Alt counted in `m`); Alt only: `\x1b\r` (**D2**; B `\r`) |
| `Backspace` | `\x7f` | Ctrl: `\x08`; Ctrl+Alt: `\x1b\x08` (**D2**; B `\x08`); Alt: `\x1b\x7f`; Shift blind |
| `Delete` | `\x1b[3~` | any: `\x1b[3;{m}~` |
| `Tab` | `\t` | Shift: `\x1b[Z`; Alt prefixes `\x1b` to either: `\x1b\t`, `\x1b\x1b[Z` (**D2**; B: Alt blind); Ctrl blind |
| `Escape` | `\x1b` | Alt: `\x1b\x1b` (**D2**; B `\x1b`); Ctrl, Shift blind |
| `ArrowUp` / `Down` / `Right` / `Left` | `\x1b[A` / `B` / `C` / `D`; `app_cursor`: `\x1bOA` .. `\x1bOD` | any: `\x1b[1;{m}A` .. `D` (DECCKM ignored) |
| `Home` / `End` | `\x1b[H` / `\x1b[F`; `app_cursor`: `\x1bOH` / `\x1bOF` | any: `\x1b[1;{m}H` / `F` |
| `PageUp` / `PageDown` | `\x1b[5~` / `\x1b[6~` | any: `\x1b[5;{m}~` / `\x1b[6;{m}~` |
| `Insert` | `\x1b[2~` | any: `\x1b[2;{m}~` (**D1**; B blind) |
| `F1` `F2` `F4` | `\x1bOP` `\x1bOQ` `\x1bOS` | any: `\x1b[1;{m}P` `Q` `S` (**D1**; B blind) |
| `F3` | `\x1bOR` | any: `\x1b[13;{m}~` (**D1**; never `\x1b[1;{m}R`, the CPR shape; B blind) |
| `F5`..`F12` | `\x1b[{n}~`, n = 15 17 18 19 20 21 23 24 | any: `\x1b[{n};{m}~` (**D1**; B blind) |
| `F13` `F14` `F15` `F16` | `\x1b[1;2P` `\x1b[1;2Q` `\x1b[28~` `\x1b[1;2S` | blind |
| `F17`..`F24` | `\x1b[{n};2~`, n = 15 17 18 19 20 21 23 24 | blind |

- `F15` is `\x1b[28~` (DEC VT220 `kf15`), not xterm's `\x1b[1;2R`: that is byte-identical to a
  Cursor Position Report (Decision 3). Invariant: no legacy output is a CSI sequence ending `R`.
- F13-F24 already spell "shifted F1-F12"; extra modifiers have no place in that form (blind).
- Shift+Enter and Ctrl+Enter send `CSI 13;m u` (kitty's legacy table sends
  `\r`); it lets shells and chat TUIs tell them from Enter with nothing negotiated.

## Worked rows (each a test, tests.md)

| Event | `app_cursor` | Bytes |
|---|---|---|
| Ctrl+`c` / Ctrl+` ` / Ctrl+Shift+`A` | - | `\x03` / `\x00` / `\x01` |
| Ctrl+`0`, `1`, `9`, `,`, `.` | - | the scalar |
| Ctrl+`é`, Ctrl+`ab` | - | `None` |
| Alt+`a` / Ctrl+Alt+`a` | - | `\x1ba` / `\x1b\x01` |
| Enter / Shift+Enter / Alt+Enter | - | `\r` / `\x1b[13;2u` / `\x1b\r` |
| Backspace / Ctrl / Alt / Ctrl+Alt | - | `\x7f` / `\x08` / `\x1b\x7f` / `\x1b\x08` |
| Delete+Ctrl | - | `\x1b[3;5~` |
| Shift+Tab / Alt+Tab / Alt+Shift+Tab | - | `\x1b[Z` / `\x1b\t` / `\x1b\x1b[Z` |
| Alt+Escape | - | `\x1b\x1b` |
| ArrowUp | off / on | `\x1b[A` / `\x1bOA` |
| Ctrl+ArrowUp, Alt+ArrowUp | on | `\x1b[1;5A`, `\x1b[1;3A` |
| Home / End | on | `\x1bOH` / `\x1bOF` |
| Ctrl+Home, Alt+Home | on | `\x1b[1;5H`, `\x1b[1;3H` |
| Ctrl+PageUp, Alt+PageUp | - | `\x1b[5;5~`, `\x1b[5;3~` |
| F1 / F5 / F12 | - | `\x1bOP` / `\x1b[15~` / `\x1b[24~` |
| Ctrl+F1 / Shift+F3 / Ctrl+F5 / Alt+Insert | - | `\x1b[1;5P` / `\x1b[13;2~` / `\x1b[15;5~` / `\x1b[2;3~` |
| F13 / F15 / F17 / F24 (any modifiers) | - | `\x1b[1;2P` / `\x1b[28~` / `\x1b[15;2~` / `\x1b[24;2~` |

## Not encoded

Keypad keys (no `NamedKey`), so `app_keypad` (DECKPAM) is unread; LNM (`20 h`) is inert in this
engine (spec 0007 modes.md), so Enter stays `\r`.
