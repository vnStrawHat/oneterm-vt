# Files to touch

Builds on specs 0007 (`KeyboardFlags`, `modify_other_keys`, mode bits) and 0008 (`ModeSnapshot`,
`MouseProtocol`, `MouseReporting`, `MouseEncoding`, `Terminal::mode_snapshot`) as implemented;
phases 6 and 7 land first. No new dependency. Sub-milestones, gate green after each:
**9a** key types, legacy rung, `Terminal` methods; **9b** kitty and `modifyOtherKeys` rungs;
**9c** mouse; **9d** `docs/user/input.md`.

## `crates/vt/src` (new)

| Path | Content | Milestone |
|---|---|---|
| `input.rs` | module doc (pure encoders, embedder owns side effects, framework-neutral types); `mod key; mod kitty; mod mouse;`; the `pub use` list below | 9a |
| `input/key.rs` | `KeyMods`, `NamedKey`, `KeySpec`, `KeyEventKind`, `KeyEvent` (+ `new`), `encode_key`, `encode_key_event`, crate `modifier_byte`, `encode_legacy`, private `ctrl_bytes` | 9a |
| `input/kitty.rs` | crate `Encoded`, `encode`; private `Form`, `named_form`, `unshifted`, `SHIFTED_ASCII`, `UNSHIFTED_ASCII`, `TEXT_SCALAR_MAX`, `kitty_applies`, `generates_text`, `csi_u`, `alternates`, `text_field`, `modify_other_keys`, `produces_control_byte` | 9b |
| `input/mouse.rs` | `TerminalMouseButton`, `MouseModifiers`, the four `encode_*` functions; private mask, terminator, coordinate writer | 9c |
| `input/key_tests.rs`, `input/kitty_tests.rs`, `input/mouse_tests.rs` | tests.md | each |

```rust
// input.rs
pub use key::{KeyEvent, KeyEventKind, KeyMods, KeySpec, NamedKey, encode_key, encode_key_event};
pub use mouse::{MouseModifiers, TerminalMouseButton, encode_mouse_move, encode_mouse_press,
                encode_mouse_release, encode_wheel_event};
```

9a ships `encode_key_event` with rung 1 reduced to "Release -> `None`" and no rung 2/3; 9b
inserts `kitty::encode` in front exactly as model.md's ladder.

## `crates/vt/src` (edits)

| Path | Change |
|---|---|
| `terminal.rs` | `pub fn encode_key(&self, key: &KeySpec, mods: KeyMods) -> Option<Vec<u8>>` and `pub fn encode_key_event(&self, event: &KeyEvent) -> Option<Vec<u8>>`, both `crate::input::..(.., &self.mode_snapshot())`; rustdoc says the bytes change once the program negotiates kitty flags or `modifyOtherKeys` |
| terminal test file (0007 `terminal_tests.rs`) | the two `Terminal` tests of tests.md |
| `lib.rs` | below |

`lib.rs` (crate doc gains: "[`input`] turns key and mouse events into the bytes the program
expects, honouring DECCKM, the kitty keyboard protocol, `modifyOtherKeys` and the mouse modes"):

```rust
pub mod input;

#[cfg(doctest)]
#[doc = include_str!("../../../docs/user/input.md")]
mod user_guide_input {}
```

Nothing from `input` is re-exported at the root (the roadmap names the items under `input::`).

## Docs

| Path | Change |
|---|---|
| `docs/user/input.md` | new chapter (tests.md outline); flip row 04 of `docs/user/README.md` to `ready` with a link |

## Rules

- No `unsafe`; no `unwrap`/`expect`/`panic!`/`unreachable!` outside tests (no `unreachable!()` in
  the arrow arm either: give each arrow its own arm or map through an `Option`).
- Every `as` cast is on a value already bounded (`bool as u8`, ASCII scalars, `Cb <= 125`);
  coordinate arithmetic saturates (`usize::MAX` row/col must not panic: add a `mouse_table` row).
- No raw control bytes in source or docs; write `\x1b`.
- `#![warn(missing_docs)]`: every public item and field documented, units and zero-basing stated.
- Output allocation: one `Vec` per call; build with `Vec::with_capacity` + `extend_from_slice` /
  `write!` into the `Vec`; no intermediate `String` per field is required (bytes are the contract).
- No bench tier (the roadmap names none for phase 9).

## Gate (beyond the quality gate)

```bash
cargo test -p oneterm-vt --no-default-features
RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features
```

## Commit

`feat(input): legacy, modifyOtherKeys, kitty keyboard and mouse encoders` (minor; body names
`Spec: docs/specs/0010-input` and the rulings on D1-D5).
