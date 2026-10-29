# Item 1: style ids are reclaimed (S1)

## Root cause

- `InternTable<Style>` never frees an id (intern.rs: "Styles and hyperlinks never move").
- `csi_mix` sends `CSI 48:2::r:g:b m` with a random colour in 10 % of its units: one new style
  per unit. The table (65,536 ids) is full after a few MiB; from then on `intern` returns
  `StyleId::DEFAULT` and counts `style_table_exhausted`, so the pen, the cells printed with it
  and the erase cell (`CSI 2K` fills with the pen's background) silently lose their colours.
- The live set is small: cells of 45 screen rows plus 10,000 history rows (one pen per text
  line of `csi_mix`, about 10,000 styles), the pens and the erase cells.

## Design

Reuse the extras mechanism unchanged: `InternTable::sweep_unreferenced` frees every id no
cell, pen, saved pen or erase cell holds; freed slots are reused by the next new styles; ids
are never renumbered, so no row is rewritten or re-stamped and snapshots see no change.
A rebuild with remap (graphemes' way) was considered and rejected: it rewrites every styled
row (up to 10,045 rows) per sweep for nothing; no `StyleId` ever leaves the engine except
through `interner_mut`, which already documents the same rule for extras.

| Item | Change |
|---|---|
| `InternTable::lookup(&self, &T) -> Option<u16>` | new, `pub(crate)`: hash hit only, no counter |
| `InternTable::room(&self) -> usize` | new, `pub(crate)`: `free.len() + (TABLE_CAPACITY - entries.len())` |
| `State::intern_pen_style(&mut self, Style) -> StyleId` | new in `terminal/sgr.rs`, the only caller is `sgr()` |
| `State::sweep_styles(&mut self)` | new in `terminal/sgr.rs`: `grid.live_style_ids(&mut live_ids)`, then `styles.sweep_unreferenced(&live_ids)` |
| `TerminalGrid::live_style_ids(&self, out)` | new, mirrors `live_extras_ids`: clear, then both screens |
| `Screen::collect_style_ids(&self, out)` | new, mirrors `collect_extras_ids`: every cell of rows flagged `STYLED` (history included; the hint has no false negatives), skipping id 0, plus `template` and `erase` of the cursor and the saved cursor |
| `State.live_extras` | renamed `live_ids`: the scratch set both sweeps share |
| `check_cell_ids` (row.rs) | a free style id is `UnknownStyle`, as a free extras id already is |

```rust
fn intern_pen_style(&mut self, style: Style) -> StyleId {
    if let Some(id) = self.interner.styles.lookup(&style) {
        return StyleId(id);
    }
    // The pen and, in `set_template`, its erase cell may each need a slot.
    if self.interner.styles.room() < 2 && self.interner.styles.needs_sweep() {
        self.sweep_styles();
    }
    self.interner.style(style)
}
```

- When: only on a miss with fewer than 2 slots left, and at least `SWEEP_INTERVAL` (4,096)
  misses since the last style sweep (`needs_sweep`, as for extras). Workloads that never fill
  the table never sweep. Measured: a fill check before every intern cost `heavy_sgr` 6 %
  (evidence.md); the lookup-first form costs one extra hash only on misses (5,377 in 32 MiB of
  `heavy_sgr`).
- Where: inside SGR dispatch, i.e. between sequences; every holder is reachable then.
- Fallback kept: when the sweep frees fewer than 2 slots (the live styles alone fill the table,
  within 4,096 of the cap), `intern` refuses as today: `StyleId::DEFAULT`, counted.
- Holders checked (none other keeps a `StyleId`): snapshots copy resolved `Style` values
  (`snapshot/row.rs`); DECRQSS reads the pen when it answers; selection, search and images hold
  none; the reflow scratch is empty between calls; `Screen.spare` is never read as content.

## Docs

| Where | New text (meaning) |
|---|---|
| `intern.rs` module doc and limits table | hyperlinks never move; style and extras ids are freed once no cell, pen or saved cursor names them, and reused |
| `Interner::style`, `Terminal::interner_mut` | minted style ids stay valid while a cell or pen holds them (as extras) |
| `FeedStats::style_table_exhausted` | SGR states that fell back to the default style because the styles in use filled the table |
| `docs/user/limits.md` styles/extras row | "a style or extras entry is freed once no cell names it" (sweep for styles: when the table is full) |
| `docs/user/cells-and-styles.md` limits note | ids keep their meaning while held; an unused style or extras id may be reused |

## Expected

`csi_mix` becomes `ok` (prototype: all four checkpoints equal the golden); speed unchanged
(55 -> 57 MiB/s pinned: the cost is SGR parsing, out of scope). `heavy_sgr`, `dense_cells`
within noise.
