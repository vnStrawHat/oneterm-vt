# Copied rows: `snapshot/row.rs`

Everything in a copied row is a **value**: interned ids index tables the render thread cannot
read after the lock is released, and the grapheme arena renumbers on a sweep. Styles, cluster
scalars and hyperlink strings are resolved while the lock is held, for changed rows only.

## Types

```rust
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StyleRun { pub cols: std::ops::Range<u16>, pub style: Style }   // half-open, resolved
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapshotContent { Scalar(char), Cluster { start: u32, len: u32 } } // span of `clusters`, in chars
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SnapshotCell {
    pub content: SnapshotContent, pub width: CellWidth, pub semantic: Semantic,
    pub run: u16,                          // index into `SnapshotRow::runs`
    pub hyperlink: Option<HyperlinkId>,    // resolve with `SnapshotState::hyperlink`
    pub graphic: Option<GraphicId>,        // resolve with `SnapshotState::placement`; None until phase 10
}
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SnapshotRow {
    pub id: RowId, pub seq: SeqNo, pub wrapped: bool,
    pub cells: Vec<SnapshotCell>,          // one per column
    pub runs: Vec<StyleRun>,
    pub clusters: Vec<char>,               // backs every `Cluster` in this row
}
impl SnapshotRow {
    pub fn style_of(&self, cell: &SnapshotCell) -> &Style;     // runs[cell.run], else &Style::DEFAULT
    pub fn cluster(&self, start: u32, len: u32) -> &[char];    // out of range (or overflow) -> &[]
    pub fn map_colors(&mut self, palette: &Palette);           // idempotent
}
```

| Field | Source |
|---|---|
| `id`, `wrapped`, `cells` | `RowRef::id`, `RowRef::wrapped`, `RowRef::cells` (a never-written slot reads as blanks) |
| `seq` | `RowRef::seq` (crate), the stamp this copy was taken at; `SeqNo(0)` for a never-written slot |
| `width`, `semantic` | `Cell::width`, `Cell::semantic` |
| `content` | `CellContent::Scalar(c)` -> `Scalar(c)`; `Grapheme(id)` -> `interner.resolve_grapheme(id)`, its `chars()` appended to `clusters`, `Cluster { start, len }` in chars |
| `hyperlink`, `graphic` | `cell.extras_id() != ExtrasId::NONE` -> `interner.resolve_extras(id)` fields; else both `None` |

## `copy_from(&mut self, row: RowRef<'_>, interner: &Interner, links: &mut FxHashMap<HyperlinkId, Hyperlink>)` (crate)

1. Set `id`, `seq`, `wrapped`; `cells.clear()`, `runs.clear()`, `clusters.clear()` (capacity kept).
2. Walk the cells left to right with `open: Option<StyleId>`. On a style id different from
   `open`: close the last run at this column, push `StyleRun { cols: col..col + 1, style:
   *interner.resolve_style(id) }`. Runs follow style **id** changes; the table deduplicates, so
   equal styles share one id and no merge pass is needed.
3. Push the cell with `run = runs.len() - 1`; close the last run at `cells.len()`.
4. A hyperlink id not yet in `links` is inserted with `interner.hyperlinks.resolve(id).cloned()`
   (an unknown id inserts nothing; `SnapshotState::hyperlink` then returns `None`).

- A uniformly styled row is one run (`0..cols`); the blank tail after text is its own run only
  when its style id differs.
- `run` fits `u16` because a row has at most `MAX_COLS` cells; cluster offsets fit `u32`.
- No allocation once the vectors reach their high-water mark (tests.md).

## Hyperlink cache

`SnapshotState.hyperlinks`: cleared on every rebuild (`Full`), extended by every copy. Bounded by
the interner's hyperlink table caps (spec 0003: 65,536 links, 4 MiB of text), because ids are
reused only after `HyperlinkTable::clear` on RIS, and RIS bumps `generation`. A map instead of the
study's `Vec` scan keeps a viewport of distinct links O(cells), not O(cells x links).

## `map_colors(&mut self, palette)`

For every run: `fg = Color::Rgb(palette.resolve(fg))`, same for `bg`, and for `underline_color`
when `Some`. Idempotent (an `Rgb` resolves to itself). Attributes are untouched: SGR 7 inverse,
bold-as-bright and `?5` reverse video stay the renderer's (`ModeSnapshot::reverse_video`).

## What a renderer does per row (rustdoc example on `SnapshotRow`)

```rust
for cell in &row.cells {
    if cell.width.is_spacer() { continue; }              // drawn by the wide cell on its left
    let style = row.style_of(cell);
    match cell.content {
        SnapshotContent::Scalar(c) => draw(c, style),
        SnapshotContent::Cluster { start, len } => draw_cluster(row.cluster(start, len), style),
    }
}
```
