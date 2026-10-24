# Input

`oneterm_vt::input` turns key and mouse events into the bytes the program expects. Every encoder
is a pure function of an event and a `ModeSnapshot`. You own the side effects (scroll to the
bottom, clear a selection, shortcuts) and writing the bytes to the PTY. Map your platform events
onto `KeySpec`, `KeyMods` and `NamedKey`.

## Example: keys

```rust
use std::time::Instant;
use oneterm_vt::input::{KeyMods, KeySpec, NamedKey, encode_key};
use oneterm_vt::{Config, EventBatch, Size, Terminal};

let mut term = Terminal::new(Size { rows: 4, cols: 20 }, Config::default());
let up = KeySpec::Named(NamedKey::ArrowUp);
let ctrl = KeyMods { ctrl: true, ..KeyMods::default() };

assert_eq!(term.encode_key(&up, KeyMods::default()).as_deref(), Some(&b"\x1b[A"[..]));
// The program turns on application cursor keys (DECCKM); the same key now sends SS3.
term.feed(b"\x1b[?1h", &mut EventBatch::new(), Instant::now());
assert_eq!(term.encode_key(&up, KeyMods::default()).as_deref(), Some(&b"\x1bOA"[..]));
// The free function takes any snapshot: copy it under your lock, encode without it.
let modes = term.mode_snapshot();
let c = KeySpec::Character("c".into());
assert_eq!(encode_key(&c, ctrl, &modes).as_deref(), Some(&b"\x03"[..]));
// `None` sends nothing (empty text, Ctrl+`é`, an unrequested release): drop the event.
assert_eq!(encode_key(&KeySpec::Character("é".into()), ctrl, &modes), None);
```

## The ladder

`encode_key_event` tries four rungs and the first that applies wins:

| Rung | Applies when | Reads |
|---|---|---|
| 1 | a release nobody asked for: `None` | `keyboard_flags` |
| 2 | a kitty flag covers this event: `CSI u`, `~` or letter form | `keyboard_flags` |
| 3 | `modifyOtherKeys` is on, Ctrl or Alt held, an "other" key: `CSI 27;m;k~` | `modify_other_keys` |
| 4 | otherwise the legacy (xterm) table | `app_cursor` |

Nothing else in `ModeSnapshot` is read; once rung 2 applies, DECCKM is ignored (`\x1b[A`, never `\x1bOA`).

## Legacy highlights

- Alt prefixes `ESC` (`Alt+a` is `\x1ba`), also on Enter, Tab, Escape and Backspace; Shift/Ctrl+Enter
  send `\x1b[13;{m}u`. `m = 1 + shift + 2*alt + 4*ctrl`.
- Modified function keys carry `m`: Ctrl+F5 is `\x1b[15;5~`, Ctrl+F1 `\x1b[1;5P`, F3 `\x1b[13;{m}~`.
- F13-F24 are fixed "shifted F1-F12" forms. F15 is `\x1b[28~`: xterm's `\x1b[1;2R` would be
  byte-identical to a cursor position report, so **no encoder emits a CSI sequence ending in `R`**.

## Kitty keyboard protocol and `modifyOtherKeys`

| Flag | Effect |
|---|---|
| `DISAMBIGUATE_ESC_CODES` | keys that produce no text use a kitty form (Enter, Tab, Backspace stay legacy) |
| `REPORT_EVENT_TYPES` | adds repeat and release; releases of non-text keys become reportable |
| `REPORT_ALTERNATE_KEYS` | adds `:shifted:base` to a `CSI u` form; inert alone |
| `REPORT_ALL_KEYS_AS_ESC` | every key, text keys, Enter, Tab and Backspace included |
| `REPORT_ASSOCIATED_TEXT` | adds `;text` to a `CSI u` form; needs `REPORT_ALL_KEYS_AS_ESC` |

`modifyOtherKeys` level 1 leaves keys with well-known behaviour alone (Tab, Ctrl+Space); level 2
sends `CSI 27;m;k~` for every Ctrl or Alt chord on a character, Enter, Tab, Backspace or Escape.

```rust
use std::time::Instant;
use oneterm_vt::input::{KeyEvent, KeyEventKind, KeyMods, KeySpec, NamedKey};
use oneterm_vt::{Config, EventBatch, Size, Terminal};

let mut term = Terminal::new(Size { rows: 4, cols: 20 }, Config::default());
let mut batch = EventBatch::new();
let escape = KeyEvent::new(KeySpec::Named(NamedKey::Escape), KeyMods::default());
term.feed(b"\x1b[>1u", &mut batch, Instant::now()); // push DISAMBIGUATE_ESC_CODES
assert_eq!(term.encode_key_event(&escape).as_deref(), Some(&b"\x1b[27u"[..]));

term.feed(b"\x1b[>10u", &mut batch, Instant::now()); // EVENT_TYPES + ALL_KEYS_AS_ESC
let mut release = KeyEvent::new(KeySpec::Character("a".into()), KeyMods::default());
release.kind = KeyEventKind::Release;
assert_eq!(term.encode_key_event(&release).as_deref(), Some(&b"\x1b[97;1:3u"[..]));
```

Fill `KeyEvent::shifted` (the character with Shift), `base_layout` (the PC-101 key at that position)
and `text` (what the event typed) when you know them. Ceilings: three modifiers, the 38 `NamedKey`s,
PC-101 un-shifting, associated text of at most 32 scalars with no control character.

## Mouse

```rust
use std::time::Instant;
use oneterm_vt::input::{MouseModifiers, TerminalMouseButton, encode_mouse_press};
use oneterm_vt::{Config, EventBatch, Size, Terminal};

let mut term = Terminal::new(Size { rows: 4, cols: 20 }, Config::default());
term.feed(b"\x1b[?1000h\x1b[?1006h", &mut EventBatch::new(), Instant::now());
let press = encode_mouse_press(
    0, 0, TerminalMouseButton::Left, term.mode_snapshot(), MouseModifiers::default(),
);
assert_eq!(press, b"\x1b[<0;1;1M");
```

`col` is `hit_test(..).0.col`; `row` is the viewport row truncated and clamped to `rows - 1` (`hit_test`
clamps to the viewport, the encoders do not). The snapshot is passed by value. An empty `Vec` means write nothing.

| `reporting` | press | release | drag | hover | wheel |
|---|---|---|---|---|---|
| `X10` (`?9`) | yes, no modifiers | no | no | no | no |
| `Normal` (`?1000`) | yes | yes | no | no | yes |
| `ButtonEvent` (`?1002`) | yes | yes | yes | no | yes |
| `AnyEvent` (`?1003`) | yes | yes | yes | yes | yes |

Encodings: `Default` (raw bytes, ceiling 223), `Utf8` (`?1005`, 2015), `Sgr` (`?1006`, none, release is
`m`), `Urxvt` (`?1015`, decimal, release is button 3); a larger coordinate clamps. A zero or NaN wheel
delta sends nothing. `mouse_reporting()` of `None` means the event is yours (selection, scrolling).

## Checklist

- Wrap pastes in `\x1b[200~` ... `\x1b[201~` when `mode_snapshot().bracketed_paste`.
- Turn the wheel into arrow keys via `encode_key` when `alt_screen && alternate_scroll && mouse.is_none()`.
- Send `\x1b[I` / `\x1b[O` on focus changes when `term.mode(Mode::FocusInOut)`.
- Write the bytes to the PTY outside your lock.
