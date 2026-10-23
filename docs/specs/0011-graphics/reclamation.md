# Extras reclamation: `intern.rs`, `grid/terminal_grid.rs`, `terminal.rs`

Every image takes a fresh `Extras { graphic: Some(id) }` entry. Without reclamation the 65,536-id
extras table fills after about 65,000 images (a Sixel animation, a thumbnail per line) and later
images are decoded but named by no cell until RIS. Spec 0003 deferred this to phase 10; 0007's
hook table lists it.

Rule: an extras id is freed only when a full read of everything that can hold one finds no
holder. Releasing a placement frees nothing: cells in history, rows split off by IL/SD, and
evicted or covered images can still name the entry, and a recycled id would turn such a cell
into an unrelated link or another image.

## `InternTable<T>` additions (all `pub(crate)`; the public API of 0003 is unchanged)

| Item | Rule |
|---|---|
| `free: Vec<u16>`, `since_sweep: u32` | new private fields; `Default` empty / 0 |
| `intern(value)` | hit -> its id. Miss: pop `free` (store `value`, index it), else push, else full -> 0 and `exhausted += 1`. Every miss, the full one included, does `since_sweep = since_sweep.saturating_add(1)` |
| `needs_sweep() -> bool` | `since_sweep >= SWEEP_INTERVAL` (private const 4,096) |
| `sweep_unreferenced(&mut self, live: &FxHashSet<u16>)` | every id `1..entries` not free and not in `live`: remove its index entry, store `T::default()`, push to `free`. Then `since_sweep = 0`. Ids are never renumbered |
| `is_free(id) -> bool` | `id != 0 && entries.get(id) == Some(&T::default())` (a value equal to the default always interns to 0, so a default at `id > 0` marks a free slot) |
| `clear()` (RIS, 0007) | also empties `free`, `since_sweep = 0` |

`entries()` never shrinks (a freed slot stays counted until reused). Styles use the same type;
nothing sweeps them, so their free list stays empty.

## Live set (`TerminalGrid`)

```rust
pub(crate) fn live_extras_ids(&self, out: &mut FxHashSet<u16>);   // clears `out` first
```

Both screens: every allocated row (history included) whose flags contain `HAS_EXTRAS` (no false
negative, 0004 paranoid.md), every cell's `extras_id().0`; plus `cursor().template()` and
`saved_cursor().template()` of each screen. Rows without the hint are skipped, so the cost is the
row walk plus the cells of rows that carry links or images.

## When it runs (`State`)

```rust
pub(crate) fn intern_extras(&mut self, value: Extras) -> ExtrasId; // sweep if due, then intern
```

- **Before** interning, never after: the value about to be interned is not in any cell yet, so
  a sweep after it would free the fresh id. Callers: `place` (the image entry) and the OSC 8 pen
  update (0007 osc.md `8` row now calls `intern_extras` instead of `interner.extras`).
- `stamp`'s merged hyperlink-and-graphic entry calls `interner.extras` directly (the row borrow
  is held); it is written to its cell in the same step.
- End of `feed`, after the placement sweep and the release drain: `needs_sweep()` -> sweep.
- A full table retries only when the interval says a sweep is due (a miss counts towards it),
  never a sweep per failed call: a table full of live ids costs O(1) per miss, not a grid walk.

## Integrity

`check_cell_ids` (0004 `grid/row.rs`): an extras id that `is_free` is `UnknownId::Extras`, like
an id past `entries()`. The tier-3 walk therefore catches a sweep that freed a held id. No other
graphics invariant is walked (a "cell id resolves to a live placement" check has legitimate
exceptions after IL splits and evictions).

## Cost

One sweep per 4,096 new extras entries. Worst case per sweep: both screens' rows plus every cell
of flagged rows (1,000,000 rows of history at most). The `sixel` bench fixture (one image per
line, 10,000 rows of history) exercises it every 4,096 images; its cost shows in tier 2.
