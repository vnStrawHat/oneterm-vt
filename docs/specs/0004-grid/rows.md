# `grid/row.rs`: rows, headers, read and write views

Module doc: a row is a fixed header plus one `Vec<Cell>` of exactly `cols` cells. A ring slot that
was never written holds no row at all and reads as blanks.

## `SeqNo` and `RowFlags`

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct SeqNo(pub u64);        // pub(crate) fn next(self) -> SeqNo  (saturating +1)

bitflags! { #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct RowFlags: u8 {
    const WRAPPED = 1 << 0;  const DIRTY = 1 << 1;  const STYLED = 1 << 2;
    const HAS_GRAPHEME = 1 << 3;  const HAS_EXTRAS = 1 << 4;  const HAS_GRAPHIC = 1 << 5;
    const CONTENT_HINTS = STYLED | HAS_GRAPHEME | HAS_EXTRAS | HAS_GRAPHIC;
} }
```

| Flag | Set by | Cleared by |
|---|---|---|
| `WRAPPED` | `set_wrapped(true)`, `wrapline` | `set_wrapped(false)`; any write/fill whose end reaches the last column; reset |
| `DIRTY` | every mutation and every re-home (moved row) | nobody in this phase (phase 7 snapshot) |
| `STYLED`, `HAS_GRAPHEME`, `HAS_EXTRAS` | a write of a cell with non-default style / grapheme content / non-`NONE` extras | reset only |
| `HAS_GRAPHIC` | `RowMut::mark_graphic` (crate, phase 10 caller) | reset only |

Content hints may be false positives, never false negatives (checked, paranoid.md).
`CONTENT_HINTS` excludes `WRAPPED` and `DIRTY` (it names hints only). Private helper
`fn hints_of(cell: Cell) -> RowFlags`.

## `RowHeader` and `Row`

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowHeader { pub id: RowId, pub seq: SeqNo, pub flags: RowFlags, pub occ: u16 }
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row { header: RowHeader, cells: Vec<Cell> }

pub fn new(id: RowId, cols: u16, seq: SeqNo, template: Cell) -> Row; // DIRTY | hints_of(template), occ 0
pub fn id(&self) -> RowId;
pub fn cells(&self) -> &[Cell];
pub fn reset(&mut self, template: Cell, seq: SeqNo);
pub fn bytes(&self) -> usize;                 // cells.capacity() * size_of::<Cell>()
pub(crate) fn recycle(spare: Option<Row>, id, cols, seq, template) -> Row; // reuse spare's Vec
```

- **`occ` rule (exact, checked):** every cell at `col >= occ` equals the row's last cell. Writes
  raise `occ` to `col + 1`, fills to `end`, ICH/DCH/shift to `cols`; `new`/`recycle`/`reset` set 0.
- **`reset`**: if `occ < len` and `cells.last() == Some(&template)`, fill only `0..occ`; else fill
  all. Then `occ = 0`, `seq`, `flags = DIRTY | hints_of(template)`. The whole-cell compare (not
  just the style id) makes the fast path exact under the `occ` rule.
- `recycle`: `clear()` + `resize(cols, template)` on the spare's `Vec` (no allocation when its
  capacity fits), fresh header. Nothing of the old row survives (test).
- `RowHeader` is `pub` for later phases (snapshot); no public method returns it in this phase.

## `RowRef<'a>` (`Clone, Copy, Debug`): what readers get

```rust
pub fn id(&self) -> RowId;          pub fn cells(&self) -> &'a [Cell];
pub fn cell(&self, col: u16) -> Cell; // out of range -> Cell::EMPTY
pub fn flags(&self) -> RowFlags;    // empty for an unwritten slot
pub fn wrapped(&self) -> bool;      pub fn occ(&self) -> u16;   // 0 unwritten
pub fn is_allocated(&self) -> bool; // false: slot never written (or trimmed id)
pub(crate) fn seq(&self) -> SeqNo;  // SeqNo(0) when unwritten
```

Fields: `id`, `header: Option<&'a RowHeader>`, `cells: &'a [Cell]`. Unwritten rows borrow a
`static BLANK_CELLS: [Cell; MAX_COLS as usize]` slice of `cols` (no allocation to read).

## `RowMut<'a>`: the one write view

`pub fn new(row: &'a mut Row, seq: SeqNo) -> RowMut<'a>` stamps `seq` and `DIRTY` on open.

```rust
pub fn id(&self) -> RowId;  pub fn cells(&self) -> &[Cell];
pub fn set(&mut self, col: u16, cell: Cell);     // validating write, below
pub fn set_wrapped(&mut self, wrapped: bool);
pub fn reset(&mut self, template: Cell, seq: SeqNo);
pub(crate) fn fill(&mut self, cols: Range<u16>, template: Cell);   // EL, ECH, ED partial
pub(crate) fn delete_cells(&mut self, col: u16, n: u16, template: Cell);  // DCH
pub(crate) fn insert_cells(&mut self, col: u16, n: u16, template: Cell);  // ICH
pub(crate) fn shift_right(&mut self, col: u16, n: u16);  // IRM before a print
pub(crate) fn repair(&mut self, col: u16, cell: Cell);  // store as given, keep WRAPPED
pub(crate) fn mark_graphic(&mut self);
```

**`set(col, cell)`** (out-of-range col: no-op):
1. Release the pair being overwritten: old cell `Wide` -> blank the `WideSpacer` at `col + 1`;
   old `WideSpacer` -> blank the `Wide` at `col - 1`. "Blank" = `Narrow`, content `' '`, style kept.
2. Store by the new cell's width: `Wide` also stores `cell.with_width(WideSpacer)` with content `' '`
   at `col + 1` after releasing that cell's own pair (at the last column the `Wide` degrades to a
   blank `Narrow`); `WideSpacer` degrades to a blank `Narrow`; `LeadingWideSpacer` off the last
   column degrades to a blank `Narrow`.
3. `occ = max(occ, end)`, add `hints_of`, clear `WRAPPED` if `end == cols` (`end` = last col written + 1).

So no sequence of public calls can produce a broken pair. `repair` (crate) stores the cell as
given for the two in-place fixes (tab cell, cross-row spacer release): keeps `WRAPPED`,
raises `occ`, adds hints; callers keep the width of the cell they replace.

**`fill`, `delete_cells`, `insert_cells`, `shift_right`**: plain slice ops (`fill`, `copy_within`),
then repair pairs only at the edges they cut (the cell before `start` and the cells at `start`,
`end - 1`, `end`), and `LeadingWideSpacer`s moved off the last column. No whole-row sweep.
Counts clamp to the row; `n == 0` changes nothing (`Screen` returns before opening the row, so nothing is stamped). `fill`/ICH/DCH add
`hints_of(template)`; all four clear `WRAPPED` when their changed range reaches the last column.
