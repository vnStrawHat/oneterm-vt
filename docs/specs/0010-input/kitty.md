# Rungs 1-2: kitty keyboard protocol (`input/kitty.rs`)

Source of truth: <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>. Flags = the live
`ModeSnapshot::keyboard_flags` (spec 0007 modes.md: `CSI > f u`, `CSI < n u`, `CSI = f;m u`).

## Flags

| Bit | Flag | Effect here |
|---|---|---|
| 1 | `DISAMBIGUATE_ESC_CODES` | every event that generates no text uses a kitty form, except Enter/Tab/Backspace |
| 2 | `REPORT_EVENT_TYPES` | adds `:2` repeat / `:3` release; makes releases of non-text keys reportable |
| 4 | `REPORT_ALTERNATE_KEYS` | adds `:shifted:base` to a `CSI u` form some other rule chose; inert alone |
| 8 | `REPORT_ALL_KEYS_AS_ESC` | every key uses a kitty form, Enter/Tab/Backspace and text keys included |
| 16 | `REPORT_ASSOCIATED_TEXT` | adds `;text` to a `CSI u` form; inert without bit 8 (spec: "undefined") |

## Predicates (exact)

```text
legacy_c0      = key is Named(Enter | Tab | Backspace)
generates_text = key is Character(_) && !(ctrl || alt)
reportable     = EVENT_TYPES && (ALL_ESC || !(legacy_c0 || generates_text))
rung 1         : kind == Release && !reportable                         -> None
rung 2 applies : flags non-empty && ( ALL_ESC
                 || (!legacy_c0 && kind != Press && reportable)
                 || (!legacy_c0 && DISAMBIGUATE && !generates_text) )
```

Consequences: Enter/Tab/Backspace keep legacy bytes (incl. modified ones) unless `ALL_ESC`, so
`reset` stays typeable after a crash; a held letter under `EVENT_TYPES` alone keeps typing it
(a repeat falls to the legacy rung, its release is `None`); a non-text repeat/release under
`EVENT_TYPES` uses a kitty form even without `DISAMBIGUATE`.

## Forms (`named_form`, kitty "Functional key definitions")

| Key | Form | Key | Form |
|---|---|---|---|
| Escape / Enter / Tab / Backspace | `CSI u` code 27 / 13 / 9 / 127 | Insert / Delete | `~` 2 / 3 |
| ArrowUp / Down / Right / Left | letter `A` / `B` / `C` / `D` | PageUp / PageDown | `~` 5 / 6 |
| Home / End | letter `H` / `F` | F1 / F2 / F4 | letter `P` / `Q` / `S` |
| F3 | `~` 13 (spec: `CSI R` removed, CPR) | F5..F12 | `~` 15 17 18 19 20 21 23 24 |
| F13..F24 | `CSI u` code 57376..57387 | `Character(s)` | `CSI u` code `unshifted(s)`; empty -> `None` |

`unshifted(s)`: first scalar `c`; if `c` is in `~!@#$%^&*()_+{}|:"<>?`, the same index of
`` `1234567890-=[]\;',./ ``; else the first scalar of `c.to_lowercase()`. Ctrl+Shift+`A` -> 97,
Ctrl+Shift+`!` -> 49. Other layouts: ceiling (Decision 5). Lock keys, keypad, media and
modifier-key codes (57358+, 57399+, 57428+, 57441+) have no `NamedKey`: never emitted.

## Fields

```text
kind   = EVENT_TYPES ? {Press: 1, Repeat: 2, Release: 3} : 1
field2 = (m, kind) == (1, 1) ? ""  :  kind == 1 ? "{m}"  :  "{m}:{kind}"     // m as model.md
~      : \x1b[{n}~               if field2 empty, else \x1b[{n};{field2}~
letter : \x1b[{X}                if field2 empty, else \x1b[1;{field2}{X}
CSI u  : \x1b[{code}{alt};{field2};{text}u, trailing empty fields dropped, inner ones kept
```

- `alt` (only with `REPORT_ALTERNATE_KEYS`): `shifted` counts only with Shift held. (s, b):
  (none, none) `""`; (s, none) `:{s}`; (s, b) `:{s}:{b}`; (none, b) `::{b}` (decimal scalars).
- `text` (only with `REPORT_ASSOCIATED_TEXT` and `REPORT_ALL_KEYS_AS_ESC`, never on Release):
  `event.text`, else the `Character` payload when neither Ctrl nor Alt is held, else none. Empty,
  more than `TEXT_SCALAR_MAX = 32` scalars, or any `char::is_control` scalar -> field dropped
  (key code still sent). Scalars as decimals joined by `:`.
- `~` and letter forms never carry `alt` or `text` (spec: only `CSI u` has them).

## Byte table (every row a test; `D` = DISAMBIGUATE, `E` = EVENT_TYPES, `A` = ALTERNATE, `K` = ALL_ESC, `T` = TEXT)

| Event | Flags | Bytes |
|---|---|---|
| Escape | D | `\x1b[27u` |
| Enter / Tab / Backspace | K | `\x1b[13u` / `\x1b[9u` / `\x1b[127u` |
| Enter / Tab / Backspace | D; D+E; D+E+A+T | `\r` / `\t` / `\x7f` |
| Enter / Tab / Backspace Release | E / E+K | `None` / `Some` |
| ArrowUp; Ctrl+Shift+ArrowUp | D | `\x1b[A`; `\x1b[1;6A` |
| ArrowUp with `app_cursor` | D | `\x1b[A` |
| ArrowUp Release | E | `\x1b[1;1:3A` |
| Ctrl+PageUp | D | `\x1b[5;5~` |
| `a` / Shift+`a` / Ctrl / Alt / Shift+Alt / Ctrl+Alt | D | `a` / `a` / `\x1b[97;5u` / `\x1b[97;3u` / `\x1b[97;4u` / `\x1b[97;7u` |
| Ctrl+Shift+`i`; Ctrl+Shift+`A` | D | `\x1b[105;6u`; `\x1b[97;6u` |
| `a`; `a` Repeat; `a` Release | K; E+K; E+K | `\x1b[97u`; `\x1b[97;1:2u`; `\x1b[97;1:3u` |
| `a` Repeat / Release | E; D+E; E+A | `a` / `None` |
| Shift+`a`, text `A` | K+T | `\x1b[97;2;65u` |
| Shift+`a`, shifted `A` | K+A | `\x1b[97:65;2u` |
| Ctrl+`a`, base `a`; Ctrl+`a`, shifted `A` | K+A | `\x1b[97::97;5u`; `\x1b[97;5u` |
| `a` + shifted `A`, base `a`, text `a` | A; T; A+T | `a` |
| `a`, text `\u{1}` / `a\u{7f}` / `a\u{9b}b` / 100 000 x `x` | K+T | `\x1b[97u` |
| `a`, text `ab` | K+T | `\x1b[97;;97:98u` |
| `a` / Shift / Ctrl / Alt / Ctrl+Alt (no `text`) | K+T | `\x1b[97;;97u` / `\x1b[97;2;97u` / `\x1b[97;5u` / `\x1b[97;3u` / `\x1b[97;7u` |
| Alt+`a`, text `å` | K+T | `\x1b[97;3;229u` |
| F13 / F15 / F17 / F24 | D | `\x1b[57376u` / `\x1b[57378u` / `\x1b[57380u` / `\x1b[57387u` |
| F3 | K | `\x1b[13~` |
| Ctrl+Space | none | `\x00` |
| `""` / Alt+`""` | all 32 sets | `None` (D3; B: `Some([])` / `Some([0x1b])` where the legacy rung answers) |
