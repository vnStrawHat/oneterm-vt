# `grid/mod.rs` and `grid/terminal_grid.rs`

## Shared value types (`grid/mod.rs`)

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct RowId(pub u64);
impl RowId { pub const PRIMARY_ORIGIN: RowId = RowId(0); pub const ALT_ORIGIN: RowId = RowId(1 << 63);
             pub fn distance(self, earlier: RowId) -> u64 /* saturating_sub */ }
impl Add<u64> for RowId; impl Sub<u64> for RowId;   // saturating
pub struct Pos { pub row: RowId, pub col: u16 }      // + Ord, Default (row-major order)
pub struct Size { pub rows: u16, pub cols: u16 }     // pub fn clamped(self) -> Size
pub struct Viewport { pub top: RowId, pub rows: u16, pub cols: u16 }
pub const SCROLLBACK_MAX: u32 = 1_000_000;  pub const DEFAULT_SCROLLBACK: u32 = 10_000;
pub(crate) const MAX_ROWS: u16 = 1024;      pub(crate) const MAX_COLS: u16 = 2048;
```

**`RowId` rules** (module doc):
- An id names a **position in the output stream**; slot = `id & mask`. Content moves between ids
  in region scrolls; a whole-screen scroll moves none. Consumers follow content with anchors.
- Ids only grow (per lane); never reset by RIS, ED 2 or ED 3. Never reused while live.
- Two lanes in one `u64` space: primary from `PRIMARY_ORIGIN`, alternate from `ALT_ORIGIN`. The
  screens grow independently, so one counter cannot keep both contiguous; lanes stay disjoint
  (`2^63` rows is centuries of output) and the integrity check asserts it.
- `distance` saturates, so an out-of-order pair gives 0, never a wrap.

`Size::clamped`: rows `1..=MAX_ROWS`, cols `1..=MAX_COLS` (sizes come from remote hosts). Every
constructor clamps. `Viewport` (= `Screen::viewport()`, crate): `{visible_top, rows, cols}`.
`ScrollRegion` also lives here (screen.md). Derives: `Clone, Copy, PartialEq, Eq, Hash, Debug`.

## `SeqNo` semantics

- One stamp **per batch**, not per mutation: `TerminalGrid::begin_batch()` bumps and returns it;
  every row mutated or re-homed inside the batch takes that stamp and `DIRTY`. A burst that
  rewrites a row 20 times costs a consumer one comparison.
- A new grid starts at `SeqNo(1)`, so `SeqNo(0)` (unwritten slot, `RowRef::seq`) is below every
  stamp a write can carry, including writes before the first `begin_batch`.
- Monotonic, saturating. Phase 7 compares row stamps to a watermark for damage.

## `TerminalGrid`

```rust
#[derive(Debug)]
pub struct TerminalGrid { primary: Screen, alt: Screen, alt_active: bool, anchors: Anchors, seq: SeqNo }
```

| Method | Vis | Meaning |
|---|---|---|
| `new(size, scrollback_limit) -> TerminalGrid` | pub | primary with the limit, alt with 0 |
| `primary()`, `alt()` | pub | `&Screen`, active or not |
| `alt_active() -> bool` | pub | |
| `screen_mut() -> &mut Screen` | pub | active screen (ops that need no anchors) |
| `begin_batch() -> SeqNo` | pub | bump, push into both screens (`seq`, `batch_lo = screen_top`) |
| `sync_anchors()` | pub | both screens' `sync_anchors` |
| `wrapline()` | pub | active screen's `wrapline` |
| `reset()` | pub | RIS: both screens `reset`, `alt_active = false`, sync, walk |
| `heap_bytes() -> usize` | pub | both screens + anchors |
| `screen()`, `screen_of(id)` | crate | active screen; the screen whose lane holds `id` |
| `linefeed, reverse_index, scroll_up, scroll_down, insert_lines, delete_lines, print, put_tab, erase_display` | crate | forward to the active screen with `&mut self.anchors` |
| `enter_alt()` | crate | no-op if active. Alt adopts the primary's region and tab stops; alt cursor = primary cursor (pens, charsets, pending wrap) at the same **screen index**; `alt_active = true` |
| `leave_alt()` | crate | no-op if not active. Primary adopts the alt's region and tab stops; `alt_active = false`. Primary cursor untouched (it was frozen) |
| `wipe()` | crate | active screen: kill its screen-row anchors, offset 0, reset every screen row with the erase cell |
| `check_integrity(Option<&Interner>) -> Result<(), IntegrityViolation>` | crate | paranoid.md |

Phase 6 composes `?47` = enter/leave; `?1047` = wipe before leave; `?1049` = `save_cursor` on the
primary + `enter_alt` + `wipe` (after enter, so the wipe uses the entering pen's background).
Region and tab stops are copied in both directions because the screens own one each while VT
semantics share one. No `resize` (phase 4), no scrollback-limit edit (phase 6).
