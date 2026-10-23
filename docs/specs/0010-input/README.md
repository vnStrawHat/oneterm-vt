# 0010: Input encoders (phase 9)

**Goal:** `input::*`: pure functions from a key or mouse event plus a `ModeSnapshot` to the bytes
the program expects. Keys: legacy (xterm) table, `modifyOtherKeys` levels 1-2, kitty keyboard
protocol (all five flags); mouse: X10/Normal/ButtonEvent/AnyEvent in Default/UTF-8/SGR/urxvt.
`Terminal::{encode_key, encode_key_event}` encode against the terminal's own modes.

**Non-goals:** keypad, lock/super/hyper/meta, media and modifier keys; focus reports (`?1004`);
paste wrapping; alternate scroll; horizontal wheel; a writer-based API; a bench tier.

| File | Covers |
|---|---|
| [model.md](model.md) | public types and signatures, output convention, `ModeSnapshot` fields read, the four-rung ladder |
| [legacy.md](legacy.md) | rung 4: control table, Alt prefix, every named key with and without modifiers, `app_cursor` |
| [modify-other-keys.md](modify-other-keys.md) | rung 3: levels 1 and 2, `CSI 27;m;k~`, which keys, the level-1 exception |
| [kitty.md](kitty.md) | rungs 1-2: each flag, applicability, `CSI u` / `~` / letter forms, event types, alternates, text, key codes |
| [mouse.md](mouse.md) | reporting filter, button codes, modifiers, four encodings, coordinate ceilings |
| [tests.md](tests.md) | byte-exact tables per (event, snapshot), test inventory, `docs/user/input.md` outline |
| [files.md](files.md) | files, `lib.rs`, `terminal.rs`, rules, gate, commit |

**Public API** (the list fixed by `docs/roadmap.md` phase 9, nothing added): model.md; `Terminal::{encode_key,
encode_key_event}`.

## Acceptance criteria

- [ ] Quality gate; `cargo test -p oneterm-vt --no-default-features`; `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features`
- [ ] Every cell of the byte tables in legacy.md, modify-other-keys.md, kitty.md, mouse.md is a test row (tests.md)
- [ ] All 70 table/unit tests of tests.md pass (32 key, 15 kitty, 23 mouse)
- [ ] No encoder emits a CSI sequence ending in `R` (CPR collision), checked over all 32 flag sets
- [ ] `docs/user/input.md` exists, row 04 flipped to ready; its Rust blocks run under `cargo test --doc`
- [ ] Commit: `feat(input): legacy, modifyOtherKeys, kitty keyboard and mouse encoders` (minor)

## Decisions (settled)

1. Pure functions, fresh `Vec` per call: keys `Option<Vec<u8>>` (`None` = send nothing), mouse `Vec<u8>` (empty = send nothing); keys take `&ModeSnapshot`, mouse takes it by value (`Copy`).
2. Ladder: unrequested release -> kitty -> `modifyOtherKeys` -> legacy; first rung that applies wins (model.md).
3. Legacy keeps: Shift/Ctrl+Enter `CSI 13;m u`; Ctrl+Shift+text = control table; F13-F24 fixed shifted forms; F15 `CSI 28~`; Ctrl+`~` = `0x1e` (kitty legacy ctrl table).
4. `app_keypad` and `LineFeedNewLine` are never read: no keypad keys exist; LNM is inert (0007), Enter is `\r`.
5. Kitty ceilings kept: three modifiers; only the 38 `NamedKey`s; un-shifted code derived via PC-101; text field inert without `REPORT_ALL_KEYS_AS_ESC`, capped at 32 scalars, dropped on any control scalar.
6. Mouse: `?9` = bare press in Default bytes whatever the encoding; Default/UTF-8 clamp coordinates at 223/2015; urxvt release = button 3.
7. Exhaustive: `KeyMods`, `MouseModifiers`, `TerminalMouseButton`; `#[non_exhaustive]`: `NamedKey`, `KeySpec`, `KeyEventKind`, `KeyEvent`.

## Design choices with alternatives (reviewer rules A = as proposed, B = the simpler alternative)

| # | Proposed (A) | Simpler alternative (B) | Evidence |
|---|---|---|---|
| D1 | F1-F12 and Insert carry the modifier: `CSI 1;m P/Q/S`, F3 `CSI 13;m~`, `CSI n;m~`, `CSI 2;m~` | drop modifiers: Ctrl+F5 = F5 = `CSI 15~` | xterm ctlseqs PC-style keys ("shift-F5 would be sent as CSI 15;2~"); kitty spec "Legacy functional keys" formula `CSI number;modifier ~` |
| D2 | Alt prefixes ESC onto Enter, Tab (`ESC \t`, `ESC CSI Z`), Escape, Ctrl+Backspace | drop Alt on those four | kitty spec legacy table: Alt+Enter `0x1b 0xd`, Alt+Tab `0x1b 0x9`, Alt+Shift+Tab `0x1b CSI Z`, Ctrl+Alt+Backspace `0x1b 0x8`; Alt+Backspace is prefixed anyway (legacy.md), so B is inconsistent |
| D3 | an empty `Character` is `None` on every rung | legacy `Some([])`, with Alt `Some([0x1b])` | `[0x1b]` is a phantom Escape press; `Some(empty)` contradicts "`None` = sends nothing" (Decision 1) |
| D4 | `encode_mouse_move` is empty under `Normal`, and under `ButtonEvent` with no button | send motion under every mode but `?9`; the caller filters | xterm ctlseqs: motion only in button-event / any-event tracking; "empty `Vec` = the mode does not report this event" stays one rule in one place |
| D5 | `encode_wheel_event` is empty when `delta_y` is `0.0` or NaN | anything not `> 0` is wheel-down (65) | a horizontal-only trackpad scroll (`delta_y == 0`) would send a spurious scroll-down |

Rows are independent: B on a row reverts only that row's table cells and tests.

## Open items

- **O1** `modifyOtherKeys` code of a shifted chord: this spec sends the un-shifted code
  (modify-other-keys.md, Ctrl+Shift+a -> `CSI 27;6;97~`); xterm may send the shifted one
  (`CSI 27;6;72~` for Ctrl+Shift+H), unverified: xterm's own pages were unreachable. Keep the
  un-shifted code until checked on a real xterm (`printf '\e[>4;2m'; cat -v`). All else was
  settled from the kitty spec, xterm ctlseqs and our specs.
