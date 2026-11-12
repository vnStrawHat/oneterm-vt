# Protocol: routing, keys, data, replies

## Routing

`apc_start(b'_')` opens a reader; the first payload byte must be `G`, otherwise the string is
ignored and counted as before. Bytes up to the first `;` are control data (at most 512 bytes),
the rest is base64 payload, decoded as it arrives. `apc_end(false)` runs the command;
`apc_end(true)` (`CAN`, `SUB`, the 16 MiB string cap) drops it and any load in flight.

## Control keys

`key=value` pairs separated by `,`. A value is a decimal `u32`, a decimal `i32` (`z`) or one
character (`a`, `t`, `o`, `d`). An unknown key is skipped; a malformed pair makes the whole
command invalid: no reply, `unhandled_sequences` grows.

| Key | Meaning | Default |
|---|---|---|
| `a` | `t` transmit, `T` transmit and place, `q` query, `p` place, `d` delete | `t` |
| `q` | `1` no `OK` reply, `2` no reply at all | `0` |
| `f` | `24` RGB, `32` RGBA, `100` PNG | `32` |
| `t` | medium; only `d` (direct) | `d` |
| `o` | `z`: the payload is a zlib stream | none |
| `m` | `1`: more chunks follow | `0` |
| `s`, `v` | pixel width and height of `f=24` and `f=32` data | required there |
| `i`, `I`, `p` | image id, image number, placement id | `0` |
| `x`, `y`, `w`, `h` | source rectangle in pixels | whole image |
| `X`, `Y` | pixel offset inside the first cell | `0` |
| `c`, `r` | columns and rows to draw over | from the pixels |
| `z` | layer; below `0` is under the text | `0` |
| `C` | `1`: the cursor does not move | `0` |
| `d` | what `a=d` deletes | `a` |
| `U`, `P`, `Q` | placeholders, relative placement | refused when set |

`S`, `O`, `H`, `V` and the animation keys are parsed and ignored.

## Chunks

The first command with `m=1` opens a load and keeps its keys. While a load is open every
`APC G` is a continuation: only its `m` and its payload count. The command runs when a chunk
without `m=1` arrives. Each chunk's base64 stands alone (`=` padding optional, leftover bits
dropped at the end of the chunk). A byte outside the base64 alphabet marks the load invalid.

## Decoding

| Step | Rule |
|---|---|
| payload | at most 64 MiB decoded bytes per load; more marks it too large |
| `o=z` | inflated with an output limit: `s * v * 3 or 4` for raw data, 64 MiB for PNG |
| `f=24`, `f=32` | `s` and `v` in `1..=4096`; at least `s * v * 3 or 4` bytes, the rest ignored; RGB gets alpha 255 |
| `f=100` | first frame; sides in `1..=4096` checked before the pixels are allocated; 16-bit samples cut to 8, palette and grey expanded, `tRNS` kept as alpha |

The result is RGBA8, row-major, straight alpha: what `GraphicData` carries.

## Replies

`APC G i=<id>[,I=<number>][,p=<placement>] ; <message> ST`, pushed like every other reply.
There is no reply when the command has neither `i` nor `I`, for `a=d`, for a chunk with `m=1`,
for `OK` under `q=1`, for anything under `q=2`. With `I` the reply carries the id the engine
gave the image.

| Message | When |
|---|---|
| `OK` | `a=q` decoded its data; `a=t` stored; `a=T` and `a=p` placed |
| `EINVAL:...` | both `i` and `I`; unknown `a`, `f`, `o`; `t` other than `d`; `U`, `P` or `Q` set; bad base64; `s` or `v` missing or past 4096; a source rectangle outside the image |
| `ENODATA:...` | fewer bytes than `s * v` needs; a zlib stream that fails or does not fit |
| `EBADPNG:...` | the PNG decoder refused the data |
| `EFBIG:...` | a load past 64 MiB |
| `ENOENT:...` | `a=p` names an image the screen's store does not hold |

A refusal also counts in `unhandled_sequences`.

## Caps

| Cap | Value | Past it |
|---|---|---|
| control data | 512 bytes | the command is invalid |
| one `APC` string | 16 MiB (parser) | aborted, the load is dropped |
| one load, decoded | 64 MiB | `EFBIG` |
| image side | 4096 px | `EINVAL` |
| stored images | 128 MiB of pixels and 1,024 images, both screens together | the oldest is forgotten |
| rows of one placement | the screen's rows, or `ceil(4096 / cell height)` if that is more | clamped |

The placement caps of Sixel (256 live, 256 MiB live, 128 MiB pending) apply unchanged.
