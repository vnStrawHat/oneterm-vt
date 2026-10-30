# S5: CSI header run and short ground runs (parser)

## Root cause

Parser alone (tier 1, `PARSE=1` runner, pinned) is 40-60 % of the grid time on the CSI-heavy
fixtures, ~2.4 ns/B against ~0.3 ns/B on plain text:

| Fixture | parser alone (MiB/s) | grid (MiB/s) | parser share |
|---|---:|---:|---:|
| `cursor_motion` (`ESC[r;cH` + 1 letter) | 403 | 243 | 60 % |
| `tui_redraw` (`ESC[r;cH` + 8 letters) | 558 | 330 | 59 % |
| `heavy_sgr` (`ESC[38;2;r;g;bm` + 1 letter) | 518 | 268 | 52 % |
| `dense_cells` (`ESC[48;5;nm` + 1 letter) | 443 | 193 | 44 % |

Per sequence the parser pays, for 1-8 bytes of text between sequences:

- `ground`: one `memchr` call to find an `ESC` that is a few bytes away (call and setup cost
  dominate for short distances);
- `ESC`: `clear()`, state `Escape`, then the next byte `[` goes through `step` -> `anywhere`
  -> `escape` -> `clear()` again -> `CsiEntry`;
- header bytes: per byte, a state compare, `csi_parameter`, a `pending_value` and `state`
  store into `self`.

## Design (technique: rio's CSI parameter run; a short scalar look-ahead before `memchr`)

| Item | Change |
|---|---|
| `Parser::ground` | look for `ESC` in the first 8 bytes with a plain loop; `memchr` only past them. An empty run before `ESC` skips `print_run` |
| `Parser::ground`, `ESC` found | when the byte after it is `[`, `clear()` and enter `CsiEntry` directly (skip `Escape`); `ESC` at the chunk end keeps today's path |
| `Parser::advance` | in `CsiEntry` / `CsiParam` call `csi_run(dispatch, rest)` first; when it stops before a byte it does not take (state still a CSI header, bytes left), fall through to today's per-byte loop for that byte and the rest of the sequence |
| `Parser::csi_run` (new, `state.rs`) | see below |

```rust
/// CSI header bytes while the state is `CsiEntry` or `CsiParam`; the value being read stays in
/// a local. The final byte dispatches; the run stops *before* any other byte.
pub(super) fn csi_run<'a, D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, bytes: &'a [u8]) -> &'a [u8]
```

- Entry: a private marker `0x3C..=0x3F` as the first byte in `CsiEntry` is collected and the
  state becomes `CsiParam` (what `header(Entry)` does).
- Loop: digit -> same rule as `digit` (full params: `mark_ignored`, value unchanged; else
  saturating `*10 +`); `;` / `:` -> `params.push(value, pending_sep)`, `pending_sep` = the
  new separator, value 0; state `CsiParam` after every consumed byte.
- `0x40..=0x7E` -> store the value, `dispatch_csi`, return the rest.
- Anything else (C0, `DEL`, intermediates, a marker after a parameter, `ESC`, `CAN`/`SUB`,
  bytes >= `0x80`) -> store the value and return the slice *starting at that byte*; the
  per-byte loop (today's table, unchanged) takes it. Chunk end -> store the value, return empty.
- `csi_run` must not call `step`. Measured: with a second call site LLVM stopped inlining
  `step` into the string states' loop and `long_osc` fell 33 % (883 -> 591), `hyperlinks`
  13 %, `osc_9_7` 10 %. Keep `step` with its single caller.
- Same calls, same `Params`, same `ignore` flag as today for every input: the helpers are
  the ones the table uses; only the loop moved.

Rejected after measuring: a 16-byte look-ahead (no gain on `tui_redraw`, 8 letters per run);
`csi_run` alone (+1-14 % parser, <3 % grid); the look-ahead alone (+5-11 % grid).

## Expected (pinned MiB/s, after S1-S4; evidence.md)

| Fixture | before (S4) | after (S5) |
|---|---:|---:|
| `cursor_motion` | 235 | 308 |
| `tui_redraw` | 302 | 381 |
| `heavy_sgr` | 263 | 288 (304 after S6) |
| `vim_session` | 206 | 256 |
| `mode_churn` | 153 | 210 |
| `csi_mix` | 124 | 156 |
| `dense_cells` | 217 | 233 |

## Risk and protection

- The chunk-split property tests (`parser_tests`, `tests/feed_chunking.rs`) compare whole
  and split feeds; tests.md adds splits inside `ESC [`, inside a parameter, at a marker and
  at a C0 byte inside a header.
- `ESC [` at the very end of a chunk, and `ESC` alone at the end, keep the `Escape` state.
