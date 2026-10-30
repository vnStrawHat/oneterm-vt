# S1 REP in runs, S6 glyph runs (text path)

## S1 root cause: REP prints one scalar at a time (`csi_mix`)

- 20 % of `csi_mix` units end in `CSI 100 b`. `repeat_preceding` calls `print_scalar` per copy:
  the full per-scalar path (charset map, width, `TerminalGrid::print`, `write_cell`,
  `advance`) 100 times. Ablation (REP made a no-op): `csi_mix` 63 -> 219 MiB/s, so REP is
  ~70 % of the fixture.

## S1 design

```rust
pub(super) fn repeat_preceding(&mut self, n: u16) {
    let Some(c) = self.state.preceding_char else { return };
    if scalar_width(c) > 0 {
        // 256-byte stack buffer filled with `c` encoded as UTF-8, fed to `print_scalars`
        // in slices of whole copies until `n` copies went through.
        ...
        return;
    }
    for _ in 0..n { self.print_scalar(c); }   // width 0 (a mark): unchanged
}
```

- `print_scalars` is the path the copies would take if they had been sent: the pending single
  shift applies to the first copy only, the GL set to the rest; line drawing maps every copy;
  wrap, insert mode and pending wrap go through the existing run/general split.
- `preceding_char` stays `c` (set by `print_scalars`). `?2027` does not matter: REP used the
  scalar path before and still does.
- A mark (width 0) keeps the per-copy loop: one `attach_marks` per copy, as today.

## S6 root cause: the general print run (`cjk_wide`, `unicode_symbols`)

Profile (sampling, cjk_wide): `RowMut::set` 51 %, of which one instruction holds 1,728 of
4,002 samples: the `occ` load right after the two `store` calls of a wide pair. `store`
updates `occ` (u16) and the flags (u8) with separate narrow stores; the next `store` loads
both as one 32-bit word, which cannot be forwarded from two smaller stores: a
store-forwarding stall per cell. Then: decode and validation 25 %, `lookup_width` 9-14 %, the
`glyph` closure called through `&F` 5-9 %.

## S6 design

| Item | Change |
|---|---|
| `RowMut::set`, `Wide` arm | write the pair as one slice store, then one `changed_up_to(index + 2, cell)` (the spacer has the wide cell's style and extras, so the hints are the same) |
| `RowMut::put_glyph(index, cell) -> bool` (new, `pub(crate)`) | writes a `Narrow` cell, or a `Wide` cell and its spacer, with no pair release and no `occ`/hint update; returns whether the last cell it overwrote was a `Wide` (whose spacer the run may orphan) |
| `RowMut::begin_run(col)` / `finish_run(end, pen, orphan)` (new, `pub(crate)`) | `release_pair(col)` before the first write; after the last: blank the `WideSpacer` at `end` when `orphan`, then one `changed_up_to(end, pen)` |
| `Screen::print_run` | ASCII fast path unchanged; the general loop moves to `print_glyph_run`, `#[inline(never)]` (inlined, its larger frame cost `tui_redraw` 6.5 % on the ASCII path) |
| `Screen::print_glyph_run` (new) | today's loop with `put_glyph`; returns 0 before any write when the first glyph is wide and needs the last column (the general path writes the filler, or drops it under `?7l`: the pair there must stay intact) |
| `mapped_glyph` (terminal/print.rs) | returns `impl Fn(char) -> Option<(char, u8)> + Copy`; call sites pass it by value, not `&glyph` |
| `Screen::print_ascii` | the printable count uses 16-byte blocks with a branch-free test, then bytes (`printable_ascii_prefix`, the parser's `ascii_prefix` technique), instead of `take_while(..).count()` |

Why only the edges: every cell of `start..end` is overwritten, so only the partner of the
first cell (a spacer whose `Wide` is at `start - 1`) and of the last overwritten cell (a `Wide`
whose spacer is at `end`) lie outside the run. 0017 measured an edge-only release that still
called `store` per cell at +3 %; the gain here comes from dropping the per-cell `occ`/flag
read-modify-write.

Why the block count is in this step: moving the general loop out of `print_run` changed the
register allocation of the byte loop that counts printable ASCII (same instructions, longer
encodings): `scroll_region` -7 %, `tui_redraw` -7 %, `plain_ascii` -4 % (best of 4 runs).
The block count removes that loop from the ASCII path and repays it: `long_lines` +33 %,
`plain_ascii` +20 %, the other two within 4 % of before S6.

## Rejected or deferred (measured)

| Idea | Result |
|---|---|
| 16 KiB BMP width table (2 bits per scalar, `LazyLock`, built from `unicode-width` on first use) | `cjk_wide` 352 -> 395 (+9 %), `unicode_symbols` 341 -> 369 (+4 %): under the bar, adds a lazily built static; see README "Later" |
| `str::from_utf8` before `utf8_chunks` in `print_run_general` | 0 % |
| single-byte print fast path in `print_scalars` | +3-4 % on `cursor_motion`, `short_lines` |
| conditional hint write to split the `occ`/flags load (`add_hints`) | 0 % on the ASCII path (`occ` is still loaded as a 32-bit word) |

## Expected (pinned MiB/s, after S1-S5; evidence.md)

| Fixture | before (S5) | after (S6) |
|---|---:|---:|
| `cjk_wide` | 215 | 353 |
| `unicode_symbols` | 214 | 352 |
| `long_lines` | 791 | 1,036 |
| `plain_ascii` | 591 (best of 4: 658) | 787 (best of 4: 790) |
| `light_cells` | 1,136 | 1,787 |
| `scrolling` | 761 | 1,246 |

S1 alone: `csi_mix` 55.7 -> 126 (HEAD -> S1 in evidence.md).
