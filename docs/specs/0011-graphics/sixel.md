# Sixel decoder: `graphics/sixel.rs`

Streaming, byte at a time, never fails, never panics. A byte the grammar has no rule for is
dropped, so a corrupt band gives wrong pixels (the documented degradation when bytes are lost).

```rust
pub(crate) struct SixelDecoder { .. }          // private fields below; manual Debug ("SixelDecoder")
impl SixelDecoder {
    pub(crate) fn new() -> SixelDecoder;
    pub(crate) fn put(&mut self, byte: u8);
    pub(crate) fn finish(self) -> Option<DecodedSixel>;   // None: nothing to place
}
pub(crate) struct DecodedSixel { pub(crate) width: u32, pub(crate) height: u32, pub(crate) rgba: Vec<u8>, pub(crate) band_pixels: u32 }
```

Fields: `palette: [[u8; 4]; 256]`, `color: u8`, `x`, `band`, measured `width`/`height` (u32),
`raster: Option<(u32, u32)>`, `state: Ground | Repeat | Color | Raster`, `args: [u32; 5]` +
`arg_count: u8`, `pixels: Vec<[u8; 4]>`, `stride: u32`.

## Routing (`terminal/dispatch.rs`, replaces 0007's "unhandled until phase 10" row)

| Call | Action |
|---|---|
| `dcs_hook(.., ignore = false, b'q')`, no intermediates | `graphics.decoder = Some(SixelDecoder::new())`; DCS params ignored (below) |
| `dcs_hook` with `$ q` / `+ q` / `ignore` / other | unchanged (0007): query, or unhandled |
| `dcs_put(b)` | decoder open -> `put(b)`; else the 0007 query rule |
| `dcs_unhook(true)` (CAN/SUB, `DCS_MAX_BYTES`) | decoder dropped: no image, no cell, cursor unmoved; `aborted_dcs += 1` (0007) |
| `dcs_unhook(false)` (ST, or the ESC that starts the next sequence) | `finish()`; `Some` -> `place` (placement.md) |

At most one sink is open: the parser always unhooks before the next hook, so an unterminated
`DCS q` followed by `ESC P $ q` is *finished* (and placed) by that ESC, not aborted.

## Grammar

| State | Byte | Action |
|---|---|---|
| Ground | `0x3F..=0x7E` | sixel `b - 0x3F`, count 1 |
| Ground | `!` / `#` / `"` | enter Repeat / Color / Raster: `args = [0; 5]`, `arg_count = 1` |
| Ground | `$` | graphics CR: `x = 0` |
| Ground | `-` | graphics NL: `x = 0`, `band = band.saturating_add(1)` |
| Ground | anything else (C0, space, digits, `;`) | ignored |
| argument | `0..=9` | `args[arg_count - 1] = sat(*10 + d)` (u32, saturating); dropped when `arg_count > 5` |
| argument | `;` | `arg_count = arg_count.saturating_add(1)` (only the first 5 are stored) |
| Repeat | other | `count = max(args[0], 1)`; a sixel byte is painted `count` times, else the repeat is dropped and the byte re-read in Ground |
| Color / Raster | other | run the command (below), then re-read the byte in Ground |

Five stored arguments is the most any command reads, so the cap changes no output; it only
bounds memory for `#1;;;;...` (a `Vec` grown per `;` would not be bounded).

## Colour (`#Pr[;Pu;Px;Py;Pz]`)

- Register = `Pr & 0xFF` (`Pr` mod 256, so no register is out of range). The command always
  selects it.
- With all five arguments: `Pu == 2` RGB percent, each `min(v, 100) * 255 / 100`; `Pu == 1` DEC
  HLS: hue 0 = blue, standard hue `h = (Px % 360 + 240) % 360` (**D1**), `L`, `S` percent clamped
  to 100, HSL to RGB, `((v + m) * 255).round().clamp(0, 255)`; any other `Pu`: select only.
  Alpha 255. Fewer than five arguments: select only.
- Default palette: VT340 registers 0-15 in percent (`0,0,0` `20,20,80` `80,13,13` `20,80,20`
  `80,20,80` `20,80,80` `80,80,20` `53,53,53` `26,26,26` `33,33,60` `60,26,26` `33,60,33`
  `60,33,60` `33,60,60` `60,60,33` `80,80,80`); 16-255 opaque black.
- A pixel takes the register's RGBA when painted; redefining the register later does not
  recolour it (no indexed image is kept).

## Raster attributes (`"Pan;Pad;Ph;Pv`)

At least four arguments and `Ph > 0 && Pv > 0` -> `raster = (min(Ph, 4096), min(Pv, 4096))`, the
last one wins, accepted anywhere in the stream; `Pan`/`Pad` ignored. The declared size is the
image size: data past it is cropped (this is what trims the last band's padding to a height that
is not a multiple of 6), area the data never reached is transparent.

## Painting

`y0 = band * 6`; `y0 >= MAX_DIMENSION` -> the sixel is dropped (x does not move). Else `x_end =
min(x + count, 4096)`, `y_end = min(y0 + 6, 4096)`; when `bits != 0` grow the buffer and set
`(x..x_end) x {y0 + i | bit i set, y < y_end}` (bit 0 = top). Then `x = x_end`, `width =
max(width, x_end)`, `height = max(height, y_end)`: a blank sixel (`?`) still extends the
measured size. Buffer: row-major, `stride` grows to `max(needed, 2 * stride)` clamped to
`64..=4096`, rows copied on growth; `pixels.len() * 4 <= MAX_DIMENSION^2 * 4` (64 MiB) by
construction (`debug_assert!`). Index with `get_mut` or a stated bound; no panicking path.

## `finish`

- Size = `raster` or the measured `(width, height)`; either axis 0 -> `None` (empty `DCS q ST`
  places nothing and leaves the cursor).
- `rgba` = `width * height * 4` bytes, row-major, stride `width * 4`, **straight alpha**; never
  painted = `[0, 0, 0, 0]`. Copy `min(width, stride)` x `min(height, buffered rows)`.
- `band_pixels = band * 6`: the height of the bands *above* the last one (cursor rule,
  placement.md). A `#` or `"` command still open at the end is run first (a `!` is dropped).
  `band_pixels` is clamped to 4096.

## Decisions

- DCS `P1` (pixel aspect) and `P3` (grid size) are ignored: pixels are square. `P2` (background
  select, DEC STD 070: `1` = keep, `0`/`2` = fill with register 0) is ignored: untouched pixels are
  always transparent. Listed as known gaps in `docs/user/conformance.md`.
- No DECSDM (`?80` stays an unknown mode: DECRQM `0`, `h`/`l` unhandled), no XTSMGRAPHICS, no
  `CSI 16 t`. Sixel scrolling behaviour (placement.md) is the only mode.

## Hostile input (one test each, tests.md)

| Input | Bound that holds |
|---|---|
| `#0!99999~` | image `4096 x 6`, `rgba.len() == 4096 * 6 * 4` |
| 700 x `-~` | rows past 4096 dropped; height 4096 |
| `"1;1;99999;99999` only | `4096 x 4096`, all transparent (64 MiB, the decode ceiling) |
| `#0;1;4294967295;50;50#0~` | no overflow (D1); a defined colour |
| `#1` + 1 MiB of `;` and digits | 5 stored arguments, no growth |
| 32 MiB of `?` after `DCS q`, no ST | parser cap aborts once: `aborted_dcs == 1`, no image, no cell |
| `CAN` inside an image | no image, no cell, cursor unmoved |
| 20,000 pseudo-random bytes `0x20..=0x7E` | no panic; every image `<= 4096` per axis, `rgba.len() == w * h * 4`; RIS then text works |
