# S1: OSC and DCS payload runs (parser)

## Root cause

String payloads go through the per-byte table: every OSC or DCS byte pays `step` (state
match), `osc_string` / `dcs_passthrough` (byte match) and one `Vec::push` (OSC) or one
`take_string_byte` + `dcs_put` call (DCS). Pinned profiles at HEAD:

| Fixture | Payload | Where the time goes |
|---|---|---|
| `long_osc` | `OSC 6 ; <8,000 letters> BEL` per line | `Terminal::feed` (the inlined per-byte loop) 94 % |
| `hyperlinks` | `OSC 8 ; id=..; <600-900 byte URI>` twice per line | `Terminal::feed` 69 % |
| `osc_9_7` | `OSC 9 ; 7 ; state=running;title=build BEL` per line | `Terminal::feed` 29 % |
| `sixel` | 480 k images of 12x12, 70 bytes each | `dcs_put` 8 %, `feed` 6 % |

Ghostty consumes an OSC payload in bulk (`stream.zig`, `consumeOscString`: a vector scan for
bytes below `0x20`, then one slice into the OSC parser); rio appends the run up to the next
control or `;` with one `extend_from_slice` and passes DCS runs as one slice
(`advance_osc_string_run`, `advance_dcs_passthrough_run`). Same technique here, without
changing the `Dispatch` trait.

## Design

| Item | Change |
|---|---|
| `Parser::ground`, `ESC` found | `ESC ]` goes straight to `OscString` (`osc.start()`), as `ESC [` goes to `CsiEntry` since 0018 |
| `Parser::advance` | after the CSI run: `OscString` -> `osc_run`, `DcsPassthrough` -> `dcs_run`; each returns the rest starting at the byte it does not take |
| `Parser::advance`, per-byte loop | also stops when the state becomes `OscString` or `DcsPassthrough` *after* its first byte (the byte a run stopped at is always the table's), so `ESC ]` split by a chunk and a DCS after its header reach the runs |
| `Parser::osc_run` (new, `state.rs`) | loop: `osc_payload_len` bytes -> `OscBuffer::extend`; a `;` -> `osc_semicolon` (unchanged); any other stop -> return |
| `osc_payload_len` (new, private fn, `state.rs`) | leading bytes that are neither `< 0x20` nor `;`, whole 16-byte blocks tested branch-free (the `ascii_prefix` idiom, vectorises on baseline x86-64) |
| `OscBuffer::extend` (new, `osc.rs`) | appends up to `cap`; anything past sets `truncated`; grows by doubling, never past `cap` (the `push` rule), so an unclaimed OSC never grows past `OSC_INLINE` |
| `Parser::dcs_run` (new, `state.rs`) | the run of bytes `dcs_put` takes (stop at `CAN`, `SUB`, `ESC`, `DEL`, `>= 0x80`); if not aborted: `dcs_put` for each byte up to the cap room, then one `dcs_unhook(true)` and `string_aborted` when bytes are left over |

```rust
// Parser::advance, the per-byte loop (prototype form)
let mut first = true;
while self.state != State::Ground
    && (first || !matches!(self.state, State::DcsPassthrough | State::OscString))
{
    first = false;
    // ... unchanged: CSI parameter shortcut, else `step`
}

pub(super) fn osc_run<'a, D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, bytes: &'a [u8]) -> &'a [u8]
pub(super) fn dcs_run<'a, D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, bytes: &'a [u8]) -> &'a [u8]
pub(super) fn extend(&mut self, bytes: &[u8])            // OscBuffer
fn osc_payload_len(bytes: &[u8]) -> usize
```

- Same `Dispatch` calls in the same order, same `OscParams`, same `truncated`, same cap
  arithmetic for every input and every chunking: the runs only take bytes the table would
  have pushed or put one by one. `osc_allows_large` is still asked at the first `;`.
- Neither run calls `step` (0018's rule: a second caller un-inlines it from the string
  states' loop). Terminators, `CAN`/`SUB`, ignored C0 bytes inside an OSC, and `DEL`/high
  bytes inside a DCS stay in the table; an ignored C0 in an OSC makes the rest of that OSC
  per-byte (rare; correct).
- The per-byte loop condition costs nothing measurable on the CSI fixtures (interleaved best
  of 3: `cursor_motion` 299 / 294, `tui_redraw` 363 / 362, `heavy_sgr` 296 / 298,
  `csi_mix` 165 / 165, `mode_churn` 206 / 208).

## Expected (pinned MiB/s, evidence.md)

| Fixture | before | after S1 |
|---|---:|---:|
| `long_osc` | 904 | 9,931 |
| `hyperlinks` | 278 | 867 |
| `osc_9_7` | 238 | 286 |
| `sixel`, `sixel_large` (best of 3) | 45.1, 26.1 | 47.3, 26.6 (+9 %, +7 % once S3/S4 remove the other costs) |

## What the runs do not do (and why)

- Skip storing the payload of an OSC nobody routes: ghostty stores nothing for an unknown
  number (`stream.zig` bulk scan, `osc_unknown_max_bytes` = 0 by default) and only looks for
  the terminator, hence its 13 GB/s. limits.md promises the 2 KiB cut and `truncated_osc`
  for every OSC, routed or not (OSC 6 is dropped by the default routes after that), and the
  parser cannot know the route without a new `Dispatch` question. Kept: after S1 the 2 KiB
  copy is the minor part of a `long_osc` line.
- Pass a DCS run to the sink as one slice: that needs a new `Dispatch` method (public API).
  Not proposed (README, "Not proposed").
