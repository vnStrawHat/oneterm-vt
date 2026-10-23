# `Dispatch` and `Parser`

Both live in `parser/mod.rs`. Rustdoc on each method states the guarantee in the right column
below (no `docs/` paths in rustdoc).

## The trait

```rust
pub trait Dispatch {
    fn print_str(&mut self, text: &str);
    fn print(&mut self, c: char) { self.print_str(c.encode_utf8(&mut [0; 4])) }
    fn execute(&mut self, byte: u8);
    fn esc(&mut self, _intermediates: &Intermediates, _ignore: bool, _byte: u8) {}
    fn csi(&mut self, _params: &Params, _intermediates: &Intermediates, _ignore: bool, _byte: u8) {}
    fn osc(&mut self, _code: Option<u32>, _params: &OscParams<'_>, _term: StringTerm, _truncated: bool) {}
    fn osc_allows_large(&self, _code: u32) -> bool { false }
    fn dcs_hook(&mut self, _params: &Params, _intermediates: &Intermediates, _ignore: bool, _byte: u8) {}
    fn dcs_put(&mut self, _byte: u8) {}
    fn dcs_unhook(&mut self, _aborted: bool) {}
    fn apc_start(&mut self, _introducer: u8) {}
    fn apc_put(&mut self, _byte: u8) {}
    fn apc_end(&mut self, _aborted: bool) {}
}
```

| Method | Called when | Guarantees |
|---|---|---|
| `print_str` | a run of printable text in Ground | non-empty; no char in `U+0000..=U+001F`, `U+007F..=U+009F`; may contain `U+FFFD` from bad input; run boundaries are **not** meaningful (chunks and controls split runs) |
| `print` | one char completed from the UTF-8 carry | same char rules; default forwards to `print_str` |
| `execute` | C0, `DEL`, C1 (`0x80..=0x9F`, raw or UTF-8 encoded) in Ground; C0 inside ESC/CSI states; CAN/SUB anywhere | never an introducer; CAN/SUB always reach it |
| `esc` | final byte of an escape | `ignore`: a third intermediate arrived |
| `csi` | final byte of a control sequence | `params.len() >= 1`; `ignore == params.ignored()` (limit hit or malformed header, D2); intermediates hold a private marker first if any |
| `osc` | BEL or ESC ends an OSC | `params.len() >= 1`; `code` per params.md; `term` = how it ended; `truncated`: bytes past the cap were dropped. Slices valid during the call only |
| `osc_allows_large` | once per OSC, when its first param closes | must depend on `code` only (a pure answer keeps chunking invariant); a memory ceiling, not policy |
| `dcs_hook` | final byte of a DCS header | same params guarantees as `csi`; payload follows |
| `dcs_put` | one payload byte | at most `DCS_MAX_BYTES` per hook; bytes `0x00..=0x17, 0x19, 0x1C..=0x7E` only |
| `dcs_unhook` | ESC, CAN/SUB, or the cap | exactly once per `dcs_hook`, except after `reset`; `aborted` = CAN/SUB or cap |
| `apc_start` | `ESC _`, `ESC X`, `ESC ^` | `introducer` is `b'_'` (APC), `b'X'` (SOS), `b'^'` (PM) |
| `apc_put` | one payload byte | at most `DCS_MAX_BYTES` per start; any byte but CAN/SUB/ESC |
| `apc_end` | ESC, CAN/SUB, or the cap | exactly once per `apc_start`, except after `reset` |

Ordering:

- Calls happen in stream order, synchronously inside `advance`; nothing is buffered across calls
  except the OSC payload and the UTF-8 carry.
- Between `dcs_hook` and `dcs_unhook` only `dcs_put` is called; same for `apc_*`. After an ESC end,
  the next call concerns the byte after ESC (usually `esc(.., b'\\')`).
- A CAN/SUB end is followed by `execute(0x18 | 0x1A)` as the very next call.

## `Parser`

```rust
pub struct Parser { /* private: state, params, pending value + sep, osc: OscBuffer,
                       string_bytes, string_aborted, utf8_carry: [u8; 4], utf8_carry_len: u8 */ }
impl Default for Parser { fn default() -> Self { Self::new() } }
impl Parser {
    pub fn new() -> Self;                                         // Ground, OSC buffer allocated
    pub fn advance<D: Dispatch + ?Sized>(&mut self, dispatch: &mut D, bytes: &[u8]);
    pub fn reset(&mut self);                                      // -> Ground, partials dropped
}
```

- `advance` never panics, never blocks, never errors; allocation is bounded by params.md.
- `reset` drops any partial sequence and the UTF-8 carry **without calling the sink**: a sink
  hooked in DCS/APC gets no end call and must reset its own string state alongside.
- `#[derive(Debug)]` is not required; do not add `Clone` (the OSC buffer makes it costly).

## Chunk invariance

For any byte stream `S` and any split of `S` into chunks, feeding the chunks in order to one
`Parser` produces the same **action log** as feeding `S` at once. The action log is the call
sequence with `print_str`/`print` expanded to one entry per char; every other call is compared by
all its arguments (params as `groups()` plus `ignored()`, intermediates as bytes, OSC params as
byte vectors). This requires `osc_allows_large` to be pure.

## UTF-8 carry

- Set only from Ground when a chunk ends inside a valid-so-far sequence (1-3 bytes).
- On the next `advance`, top up a 4-byte scratch from the new bytes and decode its first char:
  complete -> route it (`execute` for C0/C1, else `print`) and consume its remaining bytes;
  invalid -> one `U+FFFD`, consume `error_len - carry_len` new bytes (often `0`: the byte that
  broke the sequence is then re-read from Ground, so an `ESC` stays an `ESC`); still incomplete
  (tiny chunk) -> keep everything in the carry.
- Result must equal decoding the concatenated stream (tests.md has the boundary cases).
