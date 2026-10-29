# Items 4 and 6: the text path (S2, S3, S4)

## Root cause

| Layer | Cost today | Evidence |
|---|---|---|
| parser, `print_run` | `utf8_chunks` validation, then `print_text`'s branchy per-byte `find_map` for C0, DEL and `C2 80..9F` | tier 1 ~0.8 ns/B on ASCII, ~1 ns/B on CJK |
| terminal, `print_scalars` | per scalar: the `narrow` closure (charset map, `is_control`, `scalar_width` table lookup) | |
| grid, `print_narrow_run` | per cell: `RowMut::set` (pair release, width match, `store` with hint recompute, wrap clear) | grid 6.3 ns/B vs parser 0.8 |
| grid, wide glyphs | the run takes only width 1: every CJK glyph goes through `print_mapped` -> `print_glyph` -> `write_cell` -> `advance` x2 | `cjk_wide` 142 MiB/s |

## S2: bulk ASCII cell writes (grid)

- `Screen::print_narrow_run` becomes `Screen::print_run(text, mode, charset: Charset, glyph)`
  (it takes wide glyphs in S4; `TerminalGrid` forwards it) with a fast path first. Taken when
  `charset == Charset::Ascii` (the set the caller maps through; no bool flag), the existing
  bail-outs pass (insert mode, pending wrap under autowrap), and the text starts with a byte in
  `0x20..=0x7E`.
- `n` = printable-ASCII bytes within `text.as_bytes()[..min(len, cols - col)]`. The scan is
  bounded by the room on the row: unbounded, `light_cells` (7,200 letters per run) re-scanned
  the run at every row and lost 6 % instead of gaining 4x (evidence.md).
- Bail to the per-char loop when a cell in `col..min(col + n, 2)` is a wide half (the filler on
  the row above may need releasing; same rule as today).
- `RowMut::write_ascii(col, bytes, pen)` (`pub(crate)`): `release_pair(col)` and
  `release_pair(col + n - 1)` once, as `fill` does; then `cells[col + i] = base.with_ascii(b)`
  where `base = pen.with_width(Narrow).with_content(Scalar(' '))`; then
  `changed_up_to(col + n, base)` (occ, hints from the pen, `WRAPPED` cleared at the last column).
- `Cell::with_ascii(self, b: u8) -> Cell` (`pub(crate)`, `#[inline]`): sets the content bits of
  a cell whose content is blank to `b`, keeping `b' '` as the blank encoding (`b != b' '` mask).
- Cursor: `col + n < cols` -> `col + n`; else `cols - 1` with pending wrap. Returns `n` bytes.
- The first-scalar `narrow` check is skipped on this path (no width lookup for ASCII).
- Line-drawing set, non-ASCII text, and every bail-out keep the existing per-char loop.

## S3: printable-ASCII scan in the parser

- `print_run` loops: `ascii_prefix(rest)` -> `print_str` of that prefix; a C0 byte or DEL ->
  `execute`; any byte >= `0x80` -> the bytes up to the next C0 byte (or the run end) go to the
  existing general path (`utf8_chunks` + `print_text`), with `RunEnd::Byte` unless the segment
  reaches the run end, where the caller's `RunEnd` applies (only there may a character be cut
  by the chunk). A C0 byte can never sit inside a valid UTF-8 sequence, so the split changes
  no decoding.
- `ascii_prefix(bytes) -> usize`: 16-byte blocks, each tested with a branch-free
  `fold(false, |bad, &b| bad | (b.wrapping_sub(0x20) >= 0x5F))`, then a byte loop in the
  first bad block. Safe code only; the fold vectorises.
- The ASCII prefix is passed as `&str` through `str::from_utf8` (ASCII, cannot fail; a failure
  branch just falls to the general path). No `unsafe`.
- Run boundaries carry no meaning (`Dispatch::print_str` doc); the parser property test logs
  per scalar, so it stays exact.

## S4: wide glyphs in the run, quiet control scan

- The `narrow` closure becomes `glyph: impl Fn(char) -> Option<(char, u8)>`: the mapped scalar
  and its width, `None` for width 0 and controls.
- In the per-char loop of `print_run`: width 2 writes `Wide` through `RowMut::set` (spacer
  included) and advances 2; a wide glyph with `col + 1 >= cols` ends the run (the general path
  writes the `LeadingWideSpacer` and wraps); a wide glyph ending at the last column arms the
  pending wrap. The col <= 1 wide-half bail stays.
- `print_scalars`: `try_run` turns false only after a width-0 scalar (S7 then handles mark runs).
- `print_text` (parser, general segments): before the exact per-byte search, skip whole
  16-byte blocks without a byte `< 0x20`, `== 0x7F` or `== 0xC2` (`quiet_prefix`, same fold
  style). `0xC2` marks a possible `C2 80..9F` C1 control; the exact search confirms it.
- Rejected after measuring: an edge-only pair release for the run (+3 %, within noise).

## Expected (pinned MiB/s, prototype)

| Fixture | HEAD | after S2 | after S3 | after S4 |
|---|---:|---:|---:|---:|
| `plain_ascii` | 156 | 408 | 627 | 638 |
| `long_lines` | 162 | 552 | 857 | 840 |
| `scrolling` | 162 | 572 | 811 | 828 |
| `light_cells` | 176 | 681 | 1,245 | 1,205 |
| `tui_redraw` | 192 | - | - | 320 |
| `cjk_wide` | 142 | 140 | 140 | 225 |
| `unicode_symbols` | 151 | 147 | 147 | 232 |

## Risk and protection

- Wide-pair and filler invariants: `write_ascii` releases only the two edges; every interior
  cell is overwritten whole. The tier-2 integrity walk after every debug `feed`
  (`debug_check_with`) and `vt-paranoid` catch a broken pair, a bad `occ` or a missing hint.
- Hints: `write_ascii` adds the pen's hints (`STYLED`, `HAS_EXTRAS`) exactly as `store` would
  per cell; content is a scalar, so never `HAS_GRAPHEME`.
- `light_cells`, `long_lines` and `cjk_wide` cross row ends inside one run: covered by the
  wrap tests in tests.md and by the goldens.
