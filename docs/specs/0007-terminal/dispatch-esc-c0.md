# Dispatch: handler, C0/C1, ESC, print path, DCS/APC

## `Handler` (`terminal/dispatch.rs`)

```rust
pub(super) struct Handler<'a> { pub(super) state: &'a mut State, pub(super) out: &'a mut EventBatch }
impl Dispatch for Handler<'_> { .. }   // routing only; the work lives in sibling modules
```

- `execute`, `esc`, `csi`, `osc`, `dcs_hook`, `apc_start` set `dispatched` and drop the `?2027`
  carry first; `print_str` sets `dispatched`. Puts and ends touch neither.
- `unhandled()`: `stats.unhandled_sequences += 1` (saturating).
- `report(Option<ScrollReport>)`: `rows_scrolled += history_rows`, `out.push_scroll_report`.
  Every grid call that returns a report goes through it (files.md lists them).
- `osc_allows_large(code)` = `routes.allows_large(code) && routes.get(code) != OscRoute::Drop`.

## C0 and C1 (`execute`)

| Byte | Action |
|---|---|
| `07` BEL | push `Bell` |
| `08` BS | `backspace(print_mode)`: col 0 with `?45` set crosses to the previous row's last column only when that row is `WRAPPED` and the cursor row is below the region top; otherwise col 0 is a no-op (pending wrap kept); else col - 1, pending wrap cleared |
| `09` HT | `report(put_tab(1, print_mode))` |
| `0A` LF, `0B` VT, `0C` FF | `report(linefeed())`; `lines_produced += 1`; LNM never implies CR |
| `0D` CR | `carriage_return` |
| `0E` SO / `0F` SI | `shifts.active = 1` / `0` |
| `7F` DEL | nothing, not counted |
| `18` CAN, `1A` SUB | unhandled (spec 0002 D3: the cancel is what gets counted) |
| any other C0, every C1 `80..=9F` | unhandled |

`print_mode` = `PrintMode { insert: IRM, autowrap: DECAWM, reverse_wrap: ?45 }`.

## ESC (`esc`)

`ignore` -> unhandled. Then by `(final, intermediates)`:

| Seq | Action |
|---|---|
| `( B`, `) B`, `* B`, `+ B` | designate G0/G1/G2/G3 = `Ascii` (`Screen::designate_charset`) |
| `( 0`, `) 0`, `* 0`, `+ 0` | same with `SpecialCharacterAndLineDrawing`; any other set -> unhandled |
| `D` IND / `E` NEL | `report(linefeed())`, `lines_produced += 1`; NEL then CR |
| `M` RI | `report(reverse_index())` |
| `H` HTS | tab stop at the cursor column |
| `N` SS2 / `O` SS3 | `shifts.single = Some(2)` / `Some(3)` |
| `n` LS2 / `o` LS3 | `shifts.active = 2` / `3` |
| `7` DECSC / `8` DECRC | save / restore: grid cursor (pos, pens, designations, pending wrap) + `shifts.saved[screen] = (active, single)` |
| `# 8` DECALN | invalidate `EraseScreen`; every screen cell := `Cell::EMPTY` with `'E'` (default style, pen ignored) via `row_mut` + `fill`; cursor and region unchanged |
| `c` RIS | modes.md "Resets" |
| `Z` DECID | DA1 reply (queries.md) |
| `=` / `>` | `Mode::AppKeypad` set / reset |
| `\` ST | nothing, not counted (the parser already ended the string) |
| anything else | unhandled |

## Print path (`terminal/print.rs`)

**Scalar path** (`?2027` reset, the default), per `char` `c` of `print_str`:
1. `set = shifts.single.take().unwrap_or(shifts.active)`; `g = cursor.charsets[set]`.
2. `m = map_charset(g, c)`; `report(grid.print(m, scalar_width(m), print_mode, interner))`.
3. `preceding_char = Some(c)` (raw, before mapping).

`map_charset(SpecialCharacterAndLineDrawing, c)` for `c` in `0x5F..=0x7E`, else `c` unchanged:

```text
_ U+0020  ` U+25C6  a U+2592  b U+2409  c U+240C  d U+240D  e U+240A  f U+00B0  g U+00B1
h U+2424  i U+240B  j U+2518  k U+2510  l U+250C  m U+2514  n U+253C  o U+23BA  p U+23BB
q U+2500  r U+23BC  s U+23BD  t U+251C  u U+2524  v U+2534  w U+252C  x U+2502  y U+2264
z U+2265  { U+03C0  | U+2260  } U+00A3  ~ U+00B7
```

**Grapheme path** (`?2027` set). `CLUSTER_CARRY_MAX = 32` scalars. Per `print_str(text)`:
1. `joined` = `carry_text` + `text` in `scratch`; `head` = `carry_text.len()` (0 without carry).
2. With a carry: `first` = first extended grapheme cluster of `joined`. `first.len() > head`: the
   run extends it: `goto` the carry's cell (`index_of(carry.row)`; if that row left the screen,
   stay) and place `first` with `carry.charset`. Else the old cluster stands. Skip
   `max(first.len(), head)` bytes either way.
3. Each further cluster: place it with `set` taken as in the scalar path.
4. **Place**: `w = cluster_width(cluster)`; print the first scalar mapped through the set with
   width `w`, every other scalar with width 0 (they attach to that cell). `preceding_char` = last
   scalar. `w > 0`: `carry = (cursor row, (pending_wrap ? cols : cursor col) - w, set)` and
   `carry_text = cluster`, unless the cluster has more than 32 scalars: then no carry and
   `dropped_cluster_carries += 1`. `w == 0`: no carry.
5. Any non-print dispatch, `?2027` reset, RIS and `Terminal::resize` drop the carry.

**REP** (`CSI Ps b`): `preceding_char` -> `next_or(1)` times through the scalar path (wraps,
insert mode, wide handling all apply); nothing printed yet -> nothing. At most 65,535 prints.

## DCS, APC, SOS, PM

| Hook | Action |
|---|---|
| `ignore` | unhandled; payload discarded |
| `$ q` | `dcs_query = Some(Decrqss)`, `dcs_payload.clear()` |
| `+ q` | same with `Xtgettcap` |
| `q` (no intermediate) | Sixel: decoded and placed (0011) |
| anything else | unhandled |

- `dcs_put`: a query is open and `dcs_payload.len() < QUERY_MAX_BYTES` -> push; else drop.
- `dcs_unhook(aborted)`: take the query; `aborted` -> `aborted_dcs += 1` (any DCS), payload
  cleared, no answer. Else a query: payload `>= QUERY_MAX_BYTES` -> unhandled, no answer;
  else answer (queries.md). Payload `clear()`ed after, capacity kept.
- `apc_start` (APC, SOS, PM): unhandled. `apc_put`, `apc_end`: nothing.

## What counts as unhandled (`unhandled_sequences`)

- `ignore` on `esc`, `csi`, `dcs_hook`; an unknown `(final, intermediates)` key; APC/SOS/PM.
- A known sequence with a value it does not implement (`CSI 5 J`, `CSI 9 SP q`, `CSI 1 c`); for
  `SM`/`RM`, one per unknown mode number.
- C0 rows above; OSC rules in osc.md; DECRQCRA with the gate shut; XTGETTCAP and query-payload
  ceilings (queries.md); an arena overflow (events.md).
- Never: DEL, `ESC \`, an invalid DECSTBM, an odd trailing OSC 4 parameter.
