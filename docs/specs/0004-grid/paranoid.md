# Integrity checks and the `vt-paranoid` walk

Where they live: `IntegrityViolation` in `grid/integrity.rs`; `Row::check(&self, cols)` (row-local
rows of tier 2) in `row.rs`; tiers 1-2 in `grid/screen/integrity.rs` (child of `screen`, so it reads
private fields); tier 3 in `terminal_grid.rs`. Checks
**return** a violation; production code only `debug_assert!`s the result, so release builds never
panic and debug builds stop at the first broken invariant with a precise message.

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IntegrityViolation {
    IdsOutOfOrder, TooFewLiveRows, RingOverflow, HistoryOverLimit, OffsetPastHistory,
    LaneOverlap, CursorOutside { saved: bool }, RegionInvalid,
    StaleSlot { slot: usize }, RowIdMismatch { id: RowId }, RowWidth { id: RowId },
    BrokenWidePair { id: RowId, col: u16 }, LeadingSpacerInland { id: RowId, col: u16 },
    MissingHint { id: RowId, flag: RowFlags }, OccTailNotUniform { id: RowId },
    AnchorOutsideLane { id: AnchorId }, ScreenOwnedAnchors { kind: ScreenKind, live: usize },
    UnknownStyle { id: RowId, col: u16 }, UnknownExtras { id: RowId, col: u16 },
    UnknownGrapheme { id: RowId, col: u16 },
}
```

## Tier 1: bounded, always on in debug builds

`Screen::check_bounds(&self) -> Result<(), IntegrityViolation>`, O(1). Every mutating screen
method ends with `debug_assert_eq!(self.check_bounds(), Ok(()))`.

| Invariant | Violation |
|---|---|
| `oldest <= newest`; `newest - oldest + 1 >= rows` | `IdsOutOfOrder`, `TooFewLiveRows` |
| `newest - oldest < ring_len` | `RingOverflow` |
| `history_len <= scrollback_limit`; `offset <= history_len` | `HistoryOverLimit`, `OffsetPastHistory` |
| `oldest >= kind.origin()`; primary `newest < ALT_ORIGIN` | `LaneOverlap` |
| cursor and saved cursor: row in `screen_top..=newest`, `col < cols` | `CursorOutside` |
| `region.top < region.bottom <= rows` | `RegionInvalid` |

## Tier 2: the row walk

`Screen::check_rows(&self, Option<&Interner>)`, over every live id from `walk_start()` to `newest`:

| Invariant | Violation |
|---|---|
| a slot holding a row holds the row of that id (no stale rows in the live range) | `RowIdMismatch` |
| allocated row has exactly `cols` cells | `RowWidth` |
| `Wide` at `c` -> `WideSpacer` at `c + 1`; `WideSpacer` at `c` -> `Wide` at `c - 1` | `BrokenWidePair` |
| `LeadingWideSpacer` only in the last column | `LeadingSpacerInland` |
| content hints have no false negative (`STYLED`, `HAS_GRAPHEME`, `HAS_EXTRAS`) | `MissingHint` |
| every cell at `col >= occ` equals the last cell (the `reset` fast path relies on it) | `OccTailNotUniform` |
| with an interner: style id `< styles.entries()`, extras id `< extras.entries()`, grapheme resolves non-empty | `Unknown*` |
| with an interner: same for both cursors' `template` and `erase` cells | `Unknown*` |

`walk_start()`:
- **Default (bounded):** `min(batch_lo, visible_top) - 1`, clamped to `oldest..=newest`. Covers
  every row a batch can touch: rows written then scrolled into history keep ids `>= batch_lo`;
  the `- 1` covers the cross-row spacer release on the row above the screen. O(rows) per call.
- **`vt-paranoid`:** `oldest` (whole history), plus: every slot outside the live range is `None`
  or holds a row whose id is live and maps to that slot (`StaleSlot`, an O(ring) scan). With
  100,000 rows of history this is milliseconds per call: tests and fuzzing only.

## Tier 3: the grid

`TerminalGrid::check_integrity(Option<&Interner>)`: tier 1 + tier 2 on both screens; then
every live anchor's row is inside its own lane's live range (`AnchorOutsideLane`); after a
sync, each lane has exactly 3 live screen-owned anchors equal to that screen's fields
(`ScreenOwnedAnchors`).

## Where it runs

| Caller | Check |
|---|---|
| end of every mutating `Screen` method | tier 1 (`debug_assert`) |
| `TerminalGrid::reset`, `enter_alt`, `leave_alt`, `wipe` | tier 3 without interner (`debug_assert`) |
| test fixture `step` helper: after **every** op in every grid test and every proptest step | tier 3 with interner, `assert_eq!(.., Ok(()))` |
| phase 6 `Terminal::feed`, phase 7 snapshot, phase 4 resize | tier 3 (added by those phases) |

CI already runs `cargo test --workspace --all-features` on Linux, so every grid test runs with
the whole-history walk; the default `cargo test` runs the bounded one.

## Checker tests (prove the walk is not vacuous)

One test per violation the public API cannot produce, corrupting private state from the owning
module's tests: in `row_tests.rs` a `Wide` without spacer, an inland `LeadingWideSpacer`, `occ`
below a written cell, a styled cell without `STYLED`; in `screen_tests.rs` a row with a wrong id in a
live slot, `offset` past history, a cursor below `newest`, and (`#[cfg(feature = "vt-paranoid")]`) a
stale row outside the live range. Each asserts the matching `Err`.
