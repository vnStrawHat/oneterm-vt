# State machine

Paul Williams' DEC-compatible parser (vt100.net/emu/dec_ansi_parser) with the deviations marked
**D1-D6**. Private `enum State` in `parser/state.rs`; `Ground` is handled by `parser/ground.rs`.

```rust
enum State { Ground, Escape, EscapeIntermediate, CsiEntry, CsiParam, CsiIntermediate, CsiIgnore,
             DcsEntry, DcsParam, DcsIntermediate, DcsPassthrough, DcsIgnore, OscString, ApcString }
```

Byte classes: **C0x** = `0x00..=0x17 | 0x19 | 0x1C..=0x1F` (C0 minus CAN, SUB, ESC).
**clear** = empty params, intermediates, pending value, `ignore` flag.

## Anywhere rules (all states except Ground and the three string states)

| Byte | Action |
|---|---|
| `0x18` CAN, `0x1A` SUB | `execute(byte)`, open sequence discarded without dispatch, -> Ground |
| `0x1B` ESC | clear, -> Escape (so `ESC ESC [ A` is one CUU) |
| `0x80..=0xFF` | dropped, state unchanged (D1: 8-bit C1 is never an introducer) |

## Escape and EscapeIntermediate

| Byte | Escape | EscapeIntermediate |
|---|---|---|
| C0x | `execute`, stay | `execute`, stay |
| `0x20..=0x2F` | collect, -> EscapeIntermediate | collect |
| `P` `0x50` | clear, -> DcsEntry | `esc(..)`, -> Ground |
| `[` `0x5B` | clear, -> CsiEntry | `esc(..)`, -> Ground |
| `]` `0x5D` | start OSC buffer, -> OscString | `esc(..)`, -> Ground |
| `X` `^` `_` | `apc_start(byte)`, reset string count, -> ApcString | `esc(..)`, -> Ground |
| other `0x30..=0x7E` | `esc(intermediates, ignore, byte)`, -> Ground | same |
| `0x7F` | dropped | dropped |

`ignore` on `esc` is set when a third intermediate arrived (see params.md).

## CSI and DCS headers

`collect` pushes an intermediate (overflow sets `ignore`); `digit`/`sep` build params (params.md).
`dispatch` = push pending value, call `csi(params, intermediates, ignore, byte)`, -> Ground.
`hook` = push pending value, call `dcs_hook(params, intermediates, ignore, byte)`, zero the string
count, -> DcsPassthrough.

| State | C0x | `20-2F` | `30-39` | `:` | `;` | `3C-3F` | `40-7E` | `7F` |
|---|---|---|---|---|---|---|---|---|
| CsiEntry | execute | collect -> CsiInt | digit -> CsiParam | sep -> CsiParam | sep -> CsiParam | collect (marker) -> CsiParam | dispatch | drop |
| CsiParam | execute | collect -> CsiInt | digit | sep | sep | -> CsiIgnore | dispatch | drop |
| CsiIntermediate | execute | collect | -> CsiIgnore | -> CsiIgnore | -> CsiIgnore | -> CsiIgnore | dispatch | drop |
| CsiIgnore | execute | drop | drop | drop | drop | drop | dispatch with `ignore = true` (**D2**) | drop |
| DcsEntry | drop | collect -> DcsInt | digit -> DcsParam | sep -> DcsParam | sep -> DcsParam | collect (marker) -> DcsParam | hook | drop |
| DcsParam | drop | collect -> DcsInt | digit | sep | sep | -> DcsIgnore | hook | drop |
| DcsIntermediate | drop | collect | -> DcsIgnore | -> DcsIgnore | -> DcsIgnore | -> DcsIgnore | hook | drop |
| DcsIgnore | drop | drop | drop | drop | drop | drop | hook with `ignore = true` (**D2**) | drop |

- Private markers `< = > ?` are only accepted directly after `CSI`/`DCS`; they are stored as the
  first intermediate.
- **D2**: Williams' ignore states swallow the sequence silently. Here the final byte still
  dispatches once with `ignore = true` and the params collected so far, so the sink can drop and
  count it. A DCS hooked with `ignore = true` streams its payload normally; the sink discards it.

## String states

| Byte | OscString | DcsPassthrough | ApcString (APC, SOS, PM) |
|---|---|---|---|
| `0x07` BEL | `osc(.., Bel, ..)`, -> Ground | `dcs_put` | `apc_put` |
| other C0x | dropped | `dcs_put` | `apc_put` |
| CAN, SUB | OSC discarded (**D3**), `execute`, -> Ground | `dcs_unhook(true)`, `execute`, -> Ground | `apc_end(true)`, `execute`, -> Ground |
| ESC | `osc(.., St, ..)`, clear, -> Escape | `dcs_unhook(false)`, clear, -> Escape | `apc_end(false)`, clear, -> Escape |
| `;` | close OSC param (params.md) | `dcs_put` | `apc_put` |
| `0x20..=0x7E` | payload | `dcs_put` | `apc_put` |
| `0x7F..=0xFF` | payload (`0x9C` included, **D4**) | dropped (**D4**) | `apc_put` |

- The `\` after `ESC` is read in Escape and dispatches `esc` (no intermediates, `false`, `b'\\'`); sinks treat a
  bare `ESC \` as a no-op. An OSC ends at `ESC` whatever byte follows.
- **D3**: Williams runs `osc_end` on CAN/SUB; xterm cancels. Cancelled OSCs are discarded; the
  `execute(0x18 | 0x1A)` that follows is what phase 6 counts.
- **D4**: `0x9C` never terminates a string (UTF-8 streams carry it inside characters). DCS payload
  is 7-bit; high bytes are dropped. OSC and APC keep them (UTF-8 titles, kitty payloads).
- **Byte cap** (DCS and APC): after `DCS_MAX_BYTES` payload bytes, the next payload byte calls
  `dcs_unhook(true)` / `apc_end(true)` once; the rest is dropped and the terminator ends the state
  with no second end call (CAN/SUB still `execute`; ESC still enters Escape).

## Ground and the fast path (D5)

```text
advance(bytes): if a UTF-8 carry is pending, finish it first (dispatch.md, "UTF-8 carry").
  loop: Ground -> run = bytes up to memchr(0x1B) (or end); decode the run; ESC -> clear, Escape
        other states -> one byte through the tables above
```

- **D5**: one `memchr::memchr(0x1B, ..)` per ground run, not `memchr3(ESC, LF, CR)`. The run must
  be walked byte-by-byte anyway to route C0/DEL/C1 to `execute`, so stopping the SIMD scan at every
  `LF`/`CR` saves no per-byte work and restarts the scan once per line. Revisit only with tier-1
  numbers showing `memchr3` faster on `plain_ascii` and `scroll_region`.
- The run is decoded with `str::from_utf8` in a **loop over the run**: valid prefix -> print run;
  error -> replacement or C1 (below); continue after the error. Never re-scan with `memchr` per
  error: a run of 1 MiB of `0xFF` must stay linear.
- **Print run**: a valid `&str` is split only at bytes `< 0x20`, `0x7F`, and the two-byte UTF-8
  C1 sequences `C2 80..=C2 9F`. Each piece -> one `print_str`; each split point -> `execute(byte)`
  (for C1: the second byte, `0x80..=0x9F`). An ASCII-only run is one `print_str` call.

## UTF-8 decoding (D6)

| Input | Result |
|---|---|
| valid scalar `U+0020..` except `U+007F..=U+009F` | printed |
| `U+0000..=U+001F`, `U+007F`, `U+0080..=U+009F` (C1 as UTF-8) | `execute(byte value)` |
| invalid maximal subpart of length 1 whose byte is `0x80..=0x9F` | `execute(byte)` (8-bit C1) |
| any other invalid maximal subpart (`Utf8Error::error_len`) | one `print_str("\u{FFFD}")` |
| incomplete sequence cut by `ESC` | one `U+FFFD`, then Escape |
| incomplete sequence at chunk end (1-3 bytes) | carried to the next `advance` |

Maximal-subpart replacement is the WHATWG / `String::from_utf8_lossy` rule; D6 is the C1
exception (`E0 80` gives `U+FFFD` then `execute(0x80)`). UTF-8 is decoded in Ground only; OSC and
APC payloads are raw bytes.
