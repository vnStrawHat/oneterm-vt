# Rung 3: xterm `modifyOtherKeys` (`input/kitty.rs::modify_other_keys`)

Set by `CSI > 4 ; v m` (spec 0007 modes.md, `v` in `0..=2`), read as
`ModeSnapshot::modify_other_keys`. Reached only when rungs 1-2 did not answer (model.md), so a
program with kitty flags that apply never sees it.

## Rule

```text
fire iff  level != 0
     and  (ctrl || alt)                       // Shift alone never fires: the layout consumed it
     and  key has a code k (table below)
     and  !(level == 1 && legacy bytes are a control byte)
bytes  =  \x1b[27;{m};{k}~                    // m = 1 + shift + 2*alt + 4*ctrl
```

Not firing falls through to the legacy rung (legacy.md); no case returns `None` here.

| Key | Code `k` |
|---|---|
| `Character(s)` | `unshifted(s)` (kitty.md): first scalar, PC-101 un-shift for ASCII punctuation, else lower-cased; empty `s` -> no code (legacy -> `None`, D3). Open item O1 |
| `Enter` / `Tab` / `Backspace` / `Escape` | `13` / `9` / `127` / `27` |
| every other `NamedKey` (arrows, Home, F-keys, ...) | none: already unambiguous, legacy answers |

**Level-1 exception** (xterm: "except for those with well-known behavior, e.g. Tab, Backarrow
... Control-Space to make a NUL, or Control-3 to make an Escape character"), as one predicate:
the legacy encoding is `[b]` or `[0x1b, b]` with `b < 0x20 || b == 0x7f`. Level 2 has no
exception. A level other than 0 and 1 (only `2` is storable) behaves as level 2.

## Byte table (Ctrl held unless stated; every row a test)

| Key | Level 0 | Level 1 | Level 2 |
|---|---|---|---|
| `a` | `\x01` | `\x01` | `\x1b[27;5;97~` |
| `2` | `\x00` | `\x00` | `\x1b[27;5;50~` |
| `3` | `\x1b` | `\x1b` | `\x1b[27;5;51~` |
| ` ` | `\x00` | `\x00` | `\x1b[27;5;32~` |
| `;` | `;` | `\x1b[27;5;59~` | `\x1b[27;5;59~` |
| `9` | `9` | `\x1b[27;5;57~` | `\x1b[27;5;57~` |
| `é` | `None` | `\x1b[27;5;233~` | `\x1b[27;5;233~` |
| Enter | `\x1b[13;5u` | `\x1b[27;5;13~` | `\x1b[27;5;13~` |
| Tab | `\t` | `\t` | `\x1b[27;5;9~` |
| Backspace | `\x08` | `\x08` | `\x1b[27;5;127~` |
| Escape | `\x1b` | `\x1b` | `\x1b[27;5;27~` |
| ArrowUp | `\x1b[1;5A` | `\x1b[1;5A` | `\x1b[1;5A` |
| Alt+`a` (no Ctrl) | `\x1ba` | `\x1b[27;3;97~` | `\x1b[27;3;97~` |
| Alt+Enter (no Ctrl) | `\x1b\r` (D2) | `\x1b\r` (D2; B `\r`) | `\x1b[27;3;13~` |
| Ctrl+Shift+`A` | `\x01` | `\x01` | `\x1b[27;6;97~` (O1) |
| `A` plain, Shift+`A` | `A` | `A` | `A` |

## Interplay

- Kitty wins: `keyboard_flags = DISAMBIGUATE_ESC_CODES`, level 2, Ctrl+`a` -> `\x1b[97;5u`.
- A kitty flag that does not apply (e.g. only `REPORT_ALTERNATE_KEYS`) leaves rung 3 reachable.
- D2 does not change level-1 answers: Alt prefixes `ESC` onto a control byte and the predicate
  already accepts `[0x1b, b]`.
- `app_cursor` reaches this rung only through the level-1 predicate, and no key with a code has
  a DECCKM-dependent legacy form, so the answer never depends on it.
