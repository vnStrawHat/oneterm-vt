# Params, intermediates, OSC and string caps

All buffers are fixed-size except the OSC buffer. **The OSC buffer is the parser's only heap
allocation**; that invariant is what the memory tests check.

## Constants (`pub`, each with a one-line doc stating the cap and what happens past it)

```rust
pub const MAX_PARAMS: usize = 32;               // values incl. sub-params; past it: ignore
pub const MAX_INTERMEDIATES: usize = 2;         // incl. a private marker; past it: ignore
pub const MAX_OSC_PARAMS: usize = 16;           // the 16th holds the rest, `;` included
pub const OSC_INLINE: usize = 2 * 1024;         // OSC payload held without growing
pub const OSC_LARGE: usize = 8 * 1024 * 1024;   // ceiling for codes the sink claims large
pub const DCS_MAX_BYTES: usize = 16 * 1024 * 1024; // DCS / APC payload bytes, then abort
```

## `Params` (`parser/params.rs`)

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamSep { Semicolon, Colon }          // the separator *before* a value

#[derive(Clone, Debug)]
pub struct Params { values: [u16; MAX_PARAMS], seps: [ParamSep; MAX_PARAMS], len: u8, ignored: bool }
impl Params {
    pub fn len(&self) -> usize;                 // values, sub-params included
    pub fn is_empty(&self) -> bool;
    pub fn get(&self, index: usize) -> Option<u16>;      // flat index
    pub fn sep(&self, index: usize) -> Option<ParamSep>; // flat index; index 0 is Semicolon
    pub fn values(&self) -> &[u16];
    pub fn groups(&self) -> ParamGroups<'_>;    // one slice per `;` parameter
    pub fn ignored(&self) -> bool;              // a limit was hit or the header was malformed
}
pub struct ParamGroups<'a> { .. }               // Iterator<Item = &'a [u16]>; private fields
```

Crate-private: `new`, `clear`, `push(value, sep)`, `mark_ignored`, `is_full`. No public
constructor; only the parser builds `Params`.

Rules:

- A value accumulates digits as `u16`, saturating at `65535` (`CSI 99999999999 m` -> `[65535]`,
  not ignored).
- A separator pushes the pending value (`0` if no digit) and records the next value's separator.
- The final byte pushes the pending value, so `params.len() >= 1` at every `csi`/`dcs_hook`.
- `seps[0]` is always `Semicolon`. A group is a value plus the following `Colon` values.
- A digit or push while `len == MAX_PARAMS` sets `ignored`; the 32 values kept are unchanged.
- `ignored` is also set by a third intermediate and by the CSI/DCS ignore states (D2).
  The `ignore` argument of `csi`/`dcs_hook` always equals `params.ignored()`.

| Input | `groups()` | `ignored()` |
|---|---|---|
| `CSI m` | `[[0]]` | false |
| `CSI ;4m` / `CSI 4;m` | `[[0],[4]]` / `[[4],[0]]` | false |
| `CSI :1m` | `[[0,1]]` | false |
| `CSI 1:2;3m` | `[[1,2],[3]]` | false |
| `CSI 38:2::255:0:0m` | `[[38,2,0,255,0,0]]` | false |
| `CSI 38;2;255;0;0m` | `[[38],[2],[255],[0],[0]]` | false |
| `CSI ? 25 h` | `[[25]]`, intermediates `?` | false |
| 33 `;`-separated `1`s | first 32 values | true |
| `CSI 1 ? m` | `[[1]]` | true (D2) |

## `Intermediates`

```rust
#[derive(Clone, Copy, Debug)]
pub struct Intermediates { bytes: [u8; MAX_INTERMEDIATES], len: u8 }
impl Intermediates { pub fn as_slice(&self) -> &[u8]; }   // arrival order
```

`Dispatch::esc`, `csi` and `dcs_hook` take `&Intermediates` (this is why the type is public).
A third byte is not stored and calls `params.mark_ignored()`; `esc` passes `params.ignored()` too.

## OSC accumulator (`parser/osc.rs`, crate-private `OscBuffer`)

| Field | Meaning |
|---|---|
| `payload: Vec<u8>` | `Vec::with_capacity(OSC_INLINE)` in `Parser::new`; params back to back, separating `;` not stored |
| `bounds: [(u32, u32); MAX_OSC_PARAMS]`, `count` | byte ranges of the params in `payload` |
| `code: Option<u32>`, `cap: usize`, `truncated: bool` | resolved number, current cap, overflow flag |

- `start()`: clear payload, `count = 0`, `truncated = false`, `cap = OSC_INLINE`; if
  `payload.capacity() > OSC_INLINE`, `shrink_to(OSC_INLINE)` (one large OSC must not keep 8 MiB).
- `push(byte)`: if `len == cap`, set `truncated`, drop. Else, when `len == capacity`,
  `reserve_exact(capacity.min(cap - len))` (doubling, never past `cap`), then push.
- `;` closes the current param while fewer than `MAX_OSC_PARAMS - 1` params are closed; after
  that `;` is payload of the last param, so a sink can re-split it (OSC 8 URIs, OSC 4 lists).
  Once `truncated`, `;` is dropped like any byte.
- **Code**: when the first param closes (first `;` or the terminator), `code` = its bytes parsed as
  decimal if non-empty, all digits and fits `u32`, else `None`. If `Some(c)` and
  `dispatch.osc_allows_large(c)`, `cap = OSC_LARGE`. Asked at most once per OSC.
- Terminator: close the open param, call `osc(code, &params, term, truncated)`, then `start()`.
  An empty `ESC ] BEL` dispatches one empty param with `code = None`.

```rust
pub struct OscParams<'a> { payload: &'a [u8], bounds: &'a [(u32, u32)] }
impl<'a> OscParams<'a> {
    pub fn len(&self) -> usize;  pub fn is_empty(&self) -> bool;
    pub fn get(&self, index: usize) -> Option<&'a [u8]>;
    pub fn iter(&self) -> impl Iterator<Item = &'a [u8]> + '_;
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StringTerm { Bel, St }   // St = ESC; a later `\` arrives as esc
```

## DCS / APC byte cap

The parser holds no DCS/APC buffer. It keeps `string_bytes: usize` and `string_aborted: bool`;
see the byte-cap rule in [state-machine.md](state-machine.md). `DCS_MAX_BYTES` exists so the bound
holds even for a sink that forgot its own; phase 10 relies on it for Sixel.

## Fixed memory

`size_of::<Parser>() <= 512` (const assert in tests). Peak heap: `OSC_INLINE` at rest; up to
`OSC_LARGE` capacity (transient `1.5 x` during the last doubling) only while a claimed OSC is open.
