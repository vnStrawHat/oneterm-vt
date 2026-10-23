# Dispatch: CSI (`terminal/csi.rs`, `terminal/sgr.rs`)

## Arguments

- `ignore` (spec 0002 D2: limit hit, third intermediate, misplaced marker) -> unhandled, no action.
- `next_or(d)`: first value of the next `;` group; absent or `0` -> `d`. `next_raw()`: that value,
  `None` when the group is absent. Sub-parameters (`:`) are read by SGR only.
- Below: `index` = cursor row index, `col` = cursor column, `region = {top, bottom}` (0-based,
  bottom exclusive), `n = next_or(1)`, `origin` = `CursorOrigin::Region` iff DECOM is set.

## Table (key = final byte + intermediates; any other key -> unhandled)

| Seq | Name | Action |
|---|---|---|
| `@` | ICH | `insert_blanks(n)` |
| `A` | CUU | `up(n)` |
| `B`, `e` | CUD, VPR | `down(n)` |
| `C`, `a` | CUF, HPR | `goto(index, col + n)` (clamps to `cols - 1`) |
| `D` | CUB | `goto(index, col - n)` (saturating) |
| `E` / `F` | CNL / CPL | `down(n)` / `up(n)`, then column 0 |
| `G`, `` ` `` | CHA, HPA | `goto(index, n - 1)` |
| `d` | VPA | `goto_origin(n - 1, col, origin)` |
| `H`, `f` | CUP, HVP | `goto_origin(row - 1, c - 1, origin)`, `row = next_or(1)`, `c = next_or(1)` |
| `I` / `Z` | CHT / CBT | `tab_forward(n)` / `tab_backward(n)`: next / previous stop or the edge, pending wrap kept |
| `J` | ED | Ps `0/1/2/3` -> invalidate `EraseBelow/EraseAbove/EraseScreen/EraseHistory`; Ps 2, 3 push `ScreenCleared`; `report(erase_display(..))`; other Ps unhandled |
| `K` | EL | Ps `0/1/2` -> invalidate `EraseLine`, `erase_line(Right/Left/All)`; other Ps unhandled |
| `L` / `M` | IL / DL | `report(insert_lines(n))` / `report(delete_lines(n))`; DL adds `history_rows` to `lines_produced` |
| `P` / `X` | DCH / ECH | `delete_chars(n)` / `erase_chars(n)` |
| `S` / `T` | SU / SD | `report(scroll_up(region, n))`, adds `history_rows` to `lines_produced` / `report(scroll_down(region, n))` |
| `b` | REP | dispatch-esc-c0.md |
| `c`, `> c`, `= c` | DA1, DA2, DA3 | first param `0` only, else unhandled; queries.md |
| `g` | TBC | `0`: clear the stop at `col`; `3`: clear all; else unhandled |
| `? W` | DECST8C | `5`: stops back to every 8th column (`TabStops::new(cols)`); else unhandled |
| `h` `l`, `? h` `? l` | SM/RM | every param via `next_raw` -> modes.md; one unhandled per unknown number |
| `m` | SGR | below |
| `> m`, `? m` | XTMODKEYS, query | modes.md; the query replies (queries.md) |
| `n`, `? n` | DSR | queries.md |
| `! p` | DECSTR | modes.md "Resets" |
| `$ p`, `? $ p` | DECRQM | queries.md |
| `> q` | XTVERSION | first param `0` only; queries.md |
| `SP q` | DECSCUSR | modes.md |
| `r` | DECSTBM | below |
| `s` / `u` | SCOSC / SCORC | same as DECSC / DECRC |
| `t` | window ops | `14`, `18` reply (queries.md); `22` / `23` title push / pop (osc.md); else unhandled |
| `? u`, `= u`, `> u`, `< u` | kitty keyboard | modes.md; `? u` replies |
| `* y` | DECRQCRA | queries.md |

Not handled, so counted: DECSCA (`" q`), DECSED/DECSEL (`? J`, `? K`), DECSLRM, `SP k`, all others.

## Cursor motion

- `up(n)`: `floor = if index >= region.top { region.top } else { 0 }`;
  `goto(index.saturating_sub(n).max(floor), col)`.
- `down(n)`: `ceil = if index < region.bottom { region.bottom - 1 } else { rows - 1 }`;
  `goto(index.saturating_add(n).min(ceil), col)`.
- Relative moves never re-offset by the region top; only CUP, HVP and VPA are origin-relative.
  Every move clears pending wrap (`goto`), except CHT/CBT.

## DECSTBM (`r`)

`top = next_or(1)`; `bottom = next_raw()`, absent or `0` -> `rows`. `top >= bottom` -> ignored
(not counted). Else `set_region(top - 1, bottom, origin)`; it clamps `bottom` to `rows`, refuses an
empty result (ignored, not counted) and on success homes the cursor (origin-aware).

## SGR (`m`, `terminal/sgr.rs`)

Start from the pen's resolved style; walk `params.groups()`; unknown groups are skipped and the
rest still applies; then intern the style and set it on the template (content, extras, semantic
kept). "Underline := X" removes `ALL_UNDERLINES`, then inserts X.

| Group | Effect |
|---|---|
| `[0]` (incl. bare `CSI m`) | `Style::DEFAULT` |
| `[1]` `[2]` `[3]` | + BOLD, DIM, ITALIC |
| `[4]`, `[4,1]`, `[4,n>=6]` | underline := UNDERLINE |
| `[4,2]`, `[21]` | underline := DOUBLE_UNDERLINE (ECMA-48 / xterm `21`) |
| `[4,3]` `[4,4]` `[4,5]` | underline := UNDERCURL / DOTTED_UNDERLINE / DASHED_UNDERLINE |
| `[4,0]`, `[24]` | remove ALL_UNDERLINES |
| `[5]` `[6]` `[7]` `[8]` `[9]` `[53]` | + BLINK_SLOW, BLINK_FAST, INVERSE, HIDDEN, STRIKEOUT, OVERLINE |
| `[22]` `[23]` `[25]` `[27]` `[28]` `[29]` `[55]` | - BOLD+DIM, ITALIC, both blinks, INVERSE, HIDDEN, STRIKEOUT, OVERLINE |
| `[30..=37]` / `[90..=97]` | fg `Named(Black..=White)` / `Named(BrightBlack..=BrightWhite)` |
| `[40..=47]` / `[100..=107]` | bg, same mapping |
| `[39]` / `[49]` / `[59]` | fg `Named(Foreground)` / bg `Named(Background)` / underline colour `None` |
| `[38]` `[48]` `[58]` | fg / bg / underline colour from the **next groups**: `5;n` or `2;r;g;b`; a missing value or one > 255 drops the attribute after consuming what it read |
| `[38,..]` `[48,..]` `[58,..]` | colon form: `5:n`, `2:r:g:b`; more than 4 sub-values -> the one after `2` is a colour-space id and is skipped (`38:2::1:2:3`, `38:2:7:1:2:3`) |

Example: `CSI 38;2;300;1;2 m` consumes `2` and `300`, drops the colour, then applies `1` (bold)
and `2` (dim).
