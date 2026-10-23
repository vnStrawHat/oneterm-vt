# Print, tab, erase: `grid/screen/print.rs`, `grid/screen/erase.rs`

Both are child modules of `screen` holding `impl Screen` blocks (one concept each). The cursor's `template` is what a glyph
inherits; its `erase` cell is what every erase and row reset fills with. An op whose clamped
count is 0 returns before opening a row (nothing stamped).

## `PrintMode` (`pub`, in `screen/print.rs`)

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PrintMode { pub insert: bool, pub autowrap: bool }   // IRM, DECAWM
impl Default for PrintMode { /* insert: false, autowrap: true */ }
```

Passed in per call: the grid holds no mode state.

## `print(c, width: u8, mode, interner)` (crate)

The caller measures (`scalar_width`, or `cluster_width` under `?2027` in phase 6). `c.is_control()`
-> ignored. Steps:

1. `width == 0` -> **attach**: target col = cursor col if `pending_wrap`, else `col - 1` (col 0 stays
   0); a `WideSpacer` target moves one left. Cluster = target text (scalar or resolved grapheme)
   + `c`; `interner.grapheme(&cluster)`; `set` the target cell with that content (same width).
   Cursor unchanged.
2. `pending_wrap && mode.autowrap` -> `wrapline`.
3. `mode.insert && col + width < cols` -> `shift_right(col, width)` on the cursor row.
4. `width >= 2` (clamp to 2):
   - `cols < 2` -> `dropped_wide += 1`, return.
   - `col + 1 >= cols`: `!autowrap` -> `pending_wrap = true`, `dropped_wide += 1`, return;
     else write `LeadingWideSpacer` (blank, pen style) at `col`, then `wrapline`.
   - write `template.with_content(c).with_width(Wide)` (spacer placed by `set`); advance twice.
5. Else write `Narrow`; advance once.

**Write** = `set` at the cursor, plus the cross-row repair: when the overwritten cell is `Wide` or
`WideSpacer`, `col <= 1` and `id > oldest`, and the previous row's last cell is a
`LeadingWideSpacer`, that cell becomes a blank `Narrow` through `repair`, which
keeps the previous row's `WRAPPED`.

**Advance**: `col + 1 < cols` -> `col += 1`; else `pending_wrap = true`. Armed **regardless of
`DECAWM`**: turning autowrap back on over an armed flag still breaks the line.

`dropped_wide() -> u32` (`pub`): wide glyphs that could not be placed (`cols == 1`, or last column
with autowrap off). Saturating; phase 6 maps it into `FeedStats`. The wide-pair repair itself is
in `RowMut` (rows.md), so no other path can split a pair.

## `wrapline(&mut Anchors)` (`pub`)

Set `WRAPPED` on the cursor row; if `index + 1 >= region.bottom` -> `linefeed`, else row + 1;
`col = 0`; `pending_wrap = false`; `sync_anchors`. Unconditional: callers gate on `DECAWM`.

## `put_tab(count, mode)` (crate, HT)

- `pending_wrap`: if `mode.autowrap` -> `wrapline`; return either way (the tab is consumed; with
  autowrap off the flag stays armed and the cursor does not move).
- Else `count` times: if the cell at the cursor is content `' '` (any width) -> `repair` with content
  `'\t'`, same width (a spacer stays a spacer); move right to the next stop or the last column.

## Erase and in-row edits (`screen/erase.rs`, crate)

| Op | Rule |
|---|---|
| `erase_line(LineClear)` | `Right`: no-op while `pending_wrap`; else `fill(col..cols)`. `Left`: `fill(0..=col)`. `All`: `fill(0..cols)` |
| `erase_chars(n)` | ECH: `fill(col..min(col + n, cols))` |
| `delete_chars(n)` | DCH: plain shift left by `n` from `col`, tail filled with the erase cell (counts past the row clamp) |
| `insert_blanks(n)` | ICH: shift right from `col` by `n`, opened cells = erase cell |
| `erase_display(DisplayClear, anchors, &Interner) -> Option<ScrollReport>` | below |

`pub enum LineClear { Right, Left, All }` (EL 0/1/2); `pub enum DisplayClear { Below, Above, All,
Saved }` (ED 0/1/2/3). Both derive `Clone, Copy, PartialEq, Eq, Hash, Debug`.

| `DisplayClear` | Rule |
|---|---|
| `Below` | `fill(col..cols)` on the cursor row, reset every row below; `None` |
| `Above` | reset every row above (row 0 included), `fill(0..=col)` on the cursor row; `None` |
| `All`, alternate | reset every screen row; `None` |
| `All`, primary | `k` = 1 + index of the last screen row holding a non-erasable cell (0 if none); `scroll_up(full, k)`; reset rows `0..rows - k`; `Some(report)`. Content enters history; a scrolled-back view holds still |
| `Saved` | `clear_history`; `Some(report)` with `trimmed` when history was non-empty, else `None` |

Erasable cell (new `pub(crate) fn Cell::is_erasable(self, &Interner) -> bool` in `cell.rs`): content
`' '` or `'\t'`, not a spacer, no graphic in its extras, style fg/bg default and no `INVERSE`,
underline family or `STRIKEOUT`.

## `row_text(id, &GraphemeArena, &mut String)` (`pub`, in `screen.rs`)

Appends the row's text: spacer cells skipped, scalars pushed (`'\t'` cells included), grapheme
cells via `resolve` (`push_str`). Unwritten rows give `cols` spaces. No trimming, no newline.
