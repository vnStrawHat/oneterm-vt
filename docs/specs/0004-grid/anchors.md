# `grid/anchor.rs`: positions that follow content

Module doc: a `RowId` names a position, and content moves between positions (region scrolls,
later reflow). A consumer that must follow content registers an anchor and reads it back; every
row-moving primitive updates the list. The list is small (3 per screen + selection ends, marks,
placements), so a linear scan per scroll beats any index.

## Types

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AnchorId(u32);                       // private field: only the list issues ids
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AnchorKind { Cursor, SavedCursor, ViewportTop, SelectionStart, SelectionEnd,
                      Mark(u32), Graphic(GraphicId) }
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Anchor { pub kind: AnchorKind, pub pos: Pos, pub alive: bool }
#[derive(Debug, Default)]
pub struct Anchors { entries: Vec<Anchor>, free: Vec<AnchorId> }
```

`Mark(u32)` = an OSC 133 prompt mark tag (phase 6); `Graphic` = one per placement (phase 10).
`Cursor`, `SavedCursor`, `ViewportTop` are **screen-owned** (private `fn is_screen_owned(self)`).

## API

| Item | Vis | Meaning |
|---|---|---|
| `new() -> Anchors` | pub | empty list |
| `get(id) -> Option<Pos>` | pub | `None` if dead or never issued |
| `kind(id) -> Option<AnchorKind>` | pub | kind of an issued id, dead or alive |
| `iter() -> impl Iterator<Item = (AnchorId, &Anchor)>` | pub | every entry, dead included |
| `live() -> usize` | pub | live entries |
| `live_in(lane: Range<RowId>) -> usize` | pub | live entries whose row is in `lane` |
| `register(kind, pos) -> AnchorId` | crate | reuse a freed slot or push; alive |
| `set(id, pos)` | crate | move and revive (screen-owned sync) |
| `release(id)` | crate | mark dead; push to `free` only if it was not already free |
| `shift_region(rows: Range<RowId>, delta: i32, kill: Range<RowId>)` | crate | below |
| `trim(origin: RowId, oldest: RowId)` | crate | kill live anchors with row in `origin..oldest` |
| `heap_bytes() -> usize` | crate | `entries` + `free` capacity |

- `shift_region` skips dead and screen-owned entries; an anchor in `kill` dies; else one in `rows`
  moves by `delta` (saturating). Both ranges are `RowId` ranges: screens convert their region
  at the boundary, so the list knows one coordinate space.
- `trim` is **lane-scoped**: the alt screen trims on every scroll (no history) with an `oldest`
  above every primary id; a lane-blind "below oldest" would kill every primary anchor.
- Released ids are recycled: a consumer must not use an id after release. A dead entry keeps its
  slot until released, so the owner can observe that its content is gone.
- `release` dedupe: track freedom without an O(n) `free.contains` scan (e.g. a released entry is
  `alive == false` and is pushed once; `register` from `free` revives it).

## What each primitive does to anchors (not screen-owned)

| Primitive | Effect |
|---|---|
| `scroll_up`, top == 0, full region | nothing moves (ids keep content); trim kills `origin..oldest` |
| `scroll_up`, top == 0, `bottom < rows` | tail ids shift `+n`; trim as above |
| `scroll_up`, top > 0 | `kill` = first `n` region ids; rest of the region shifts `-n` |
| `scroll_down` | `kill` = last `n` region ids; rest shifts `+n` |
| IL / DL | as the `scroll_down` / `scroll_up` they call |
| `clear_history` (ED 3), `Screen::reset` | trim to the new `oldest` |
| `wipe` (alt entry clear) | kill every anchor on the screen rows (content context replaced) |
| EL, ECH, DCH, ICH, ED 0/1/2, print, tab | nothing: erase never kills; phase 5 decides selection invalidation |
| ED 2 on the primary | the scrolled rows keep their ids in history; anchors stay alive with them |

`RowsScrolled` in a `ScrollReport` always equals the shift the anchors took (test).

## Screen-owned anchors

Each `Screen` registers `Cursor`, `SavedCursor`, `ViewportTop` in `new` and never releases them.
The fields on `Screen` are the authority during an op (the cursor keeps its screen index on a
scroll, where content anchors follow content); `Screen::sync_anchors(&self, &mut Anchors)` writes
`cursor.pos`, `saved_cursor.pos`, `(visible_top, 0)` back at the end of every op that takes
`&mut Anchors`. Ops that take no anchors (`goto`, `scroll_viewport`, ...) leave the entries stale
until the next sync; `TerminalGrid::sync_anchors` exists for that. Invariant (paranoid.md):
exactly 3 live screen-owned anchors per lane, equal to the fields after a sync. Phase 4 reflow
adds the read-back (`remap`).
