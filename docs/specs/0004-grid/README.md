# 0004: Grid (phase 3)

## Goal

`oneterm_vt::grid`: rows of `Cell`s in a ring-buffer scrollback, primary and alternate `Screen`s,
cursor, tab stops, scroll region, print path, erase/insert/delete, anchors that follow content,
stable `RowId`s and per-batch `SeqNo` stamps: every primitive the phase 6 dispatcher will call.

## Non-goals

- Resize/reflow (phase 4), selection (5), dispatch, modes, charset translation, reverse wrap,
  `DECSCA` selective erase, CUU/CUD clamps, scrollback-limit edits, line counting (6), damage
  snapshots (7), graphics placements (10). Caps on consumer anchors: none are registered yet.

## Files in this spec

| File | Covers |
|---|---|
| [rows.md](rows.md) | `SeqNo`, `RowFlags`, `RowHeader`, `Row`, `RowRef`, `RowMut`, `occ`, pair repair |
| [screen.md](screen.md) | ring, ids, viewport, cursor, region, tabs, scroll primitives, IL/DL, reset |
| [screen-edit.md](screen-edit.md) | print path, `wrapline`, tab, EL/ED/ECH/DCH/ICH, `row_text` |
| [anchors.md](anchors.md) | `Anchors`, `Anchor`, `AnchorKind`, move/kill rules |
| [terminal-grid.md](terminal-grid.md) | `TerminalGrid`, alt screen, `Size`, `Pos`, `Viewport`, `RowId`, `SeqNo` |
| [paranoid.md](paranoid.md) | integrity tiers, bounded vs `vt-paranoid` walk, where they run |
| [tests.md](tests.md) | unit tests, proptest, allocation test, user chapter `grid.md` |
| [files.md](files.md) | files, `lib.rs`, `cell.rs` addition, lint attribute, commit subject |

## Public API (names fixed)

`grid::*` (29 items, listed in [files.md](files.md); topic files give each type's pub
methods); root `Pos, RowId, RowRef, SeqNo, Size, Viewport`. All else `pub(crate)` or private.

## Acceptance criteria

- [ ] Quality gate; `cargo test -p oneterm-vt` with `--no-default-features` and with
      `--features vt-paranoid`; `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features`
- [ ] Every test in [tests.md](tests.md) passes, incl. proptest and the no-allocation test
- [ ] `docs/user/grid.md` exists; its Rust blocks run under `cargo test --doc`
- [ ] No `unsafe`, no `unwrap`/`expect`/`panic!`/`unreachable!` outside tests; no extra `pub` item
- [ ] Commit subject: `feat(grid): ring-buffer screens with scrollback, anchors and stable row ids`

## Decisions

1. `RowId` = stream position (slot `id & mask`), never reset; lanes from 0 and `1 << 63`.
2. Ring sized once, `(scrollback + 1024).next_power_of_two()` slots of 48 B; rows lazy.
3. `SeqNo` per batch (`begin_batch`), not per mutation; a grid starts at `SeqNo(1)`.
4. Full-screen scroll moves no content (`scrolled: None`); region scrolls rotate slots by swap.
5. `RowMut::set` keeps wide pairs well formed: no public call can break the pair invariant.
6. Exact `occ`: cells at `col >= occ` all equal the last cell; checked by the walk.
7. Print path lands here (`PrintMode` is `grid::*`); the caller passes the width.
8. Pending wrap arms at the last column regardless of `DECAWM`; EL 0 is a no-op while armed.
9. ED 2 on the primary scrolls occupied rows into history; DL at row 0 feeds history too.
10. Erase never kills anchors; scroll-out, trim, ED 3 and the alt `wipe` do.
11. Alt screen: idempotent `enter_alt` / `leave_alt` + `wipe`; phase 6 composes 47/1047/1049.

## Open items (owner)

1. Crate-private ops have no non-test caller until phase 6; spec uses a self-removing
   `cfg_attr(not(test), expect(dead_code, ..))` (files.md) despite "never `allow(dead_code)`".
   Confirm, or choose: make the ops `pub` (wider API), or move them to phase 6.
