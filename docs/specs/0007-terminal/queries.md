# Queries and replies: `terminal/query.rs`

Every reply is pushed as `VtEvent::Reply` and is **API**: changing a byte is a major bump
(docs/user/versioning.md). `ESC` = `1B`, `ST` = `ESC \`, `{x}` = decimal unless stated.

## Reply table (one byte-exact integration test per row, tests.md)

| Query | Bytes in | Reply |
|---|---|---|
| DA1 | `CSI c`, `CSI 0 c`, `ESC Z` | `ESC [ ? 6 2 ; 4 ; 2 2 c` (VT220, Sixel, ANSI colour) |
| DA2 | `CSI > c`, `CSI > 0 c` | `ESC [ > 0 ; {v} ; 1 c`, `v` below (engine 0.1.0 -> `100`) |
| DA3 | `CSI = c`, `CSI = 0 c` | `ESC P ! \| 0 0 0 0 0 0 0 0 ST` |
| DSR status | `CSI 5 n` | `ESC [ 0 n` |
| CPR | `CSI 6 n` | `ESC [ {row} ; {col} R`, 1-based; with DECOM `row` is relative to the region top |
| DECXCPR | `CSI ? 6 n` | `ESC [ ? {row} ; {col} ; 1 R` (same row rule) |
| DECRQM ANSI | `CSI {m} $ p` | `ESC [ {m} ; {s} $ y` (`m` echoed, `s` per modes.md; unknown -> `0`) |
| DECRQM private | `CSI ? {m} $ p` | `ESC [ ? {m} ; {s} $ y` |
| XTVERSION | `CSI > q`, `CSI > 0 q` | `ESC P > \| {name} ST`; default name `oneterm-vt(0.1.0)` |
| kitty flags | `CSI ? u` | `ESC [ ? {live flags} u` |
| modifyOtherKeys | `CSI ? 4 m` | `ESC [ > 4 ; {level} m` |
| text area px | `CSI 14 t` | `ESC [ 4 ; {rows * cell_h} ; {cols * cell_w} t` of the effective cell: `cell_pixels`, or `VIRTUAL_CELL` (10 x 20) when either axis is 0 (phase 10, 0011 D4) |
| text area cells | `CSI 18 t` | `ESC [ 8 ; {rows} ; {cols} t` |
| DECRQCRA | `CSI {id};{pg};{t};{l};{b};{r} * y` | `ESC P {id} ! ~ {HHHH} ST`, gated, below |
| DECRQSS | `DCS $ q {req} ST` | `ESC P 1 $ r {value}{req} ST` or `ESC P 0 $ r ST`, below |
| XTGETTCAP | `DCS + q {hex}[;{hex}].. ST` | one reply per name, below |
| OSC colour | `OSC 4;n;?`, `10..12;?`, `17;?`, `19;?` | engine pushes `ColorQuery`; bytes via `ColorKey::color_reply` (colors.md) |

Reported numbers are the engine's state at the query, in stream order (two queries in one feed
-> two replies, in order).

## Product name (`Config::product_name`)

- **Sanitise once in `Terminal::new`**: drop every char in `U+0000..=U+001F` and
  `U+007F..=U+009F`; append chars while the byte length stays `<= 64` and stop at the first that
  does not fit (never skip to a shorter one); stored as `Some(Cow::Owned(..))`, empty result -> `None`.
- XTVERSION `name` = the sanitised name, else `oneterm-vt(<CARGO_PKG_VERSION>)`.
- DA2 `v`: if the name ends `)` and holds `(`, `inner` = text after the last `(` minus `)`; its
  first `.` part must parse as `u32`, else use the engine version. `version_number(s)`: split on
  `.`, each of the first three parts parsed as `u32` (failure -> 0) then `min(99)`;
  `major * 10000 + minor * 100 + patch`. `Acme(1.4.0)` -> `10400`; `P(7.4294967296.9)` ->
  `70009`; `P(-1.2.3)`, `P()`, `P(4294967296.0.0)` -> engine.

## DECRQCRA (`CSI Pid ; Pp ; Pt ; Pl ; Pb ; Pr * y`)

1. `id = next_or(0)`; `Pp` read and ignored (one page). `(min, max)` = `(region.top,
   region.bottom)` with DECOM, else `(0, rows)`. `t = next_or(1)`, `l = next_or(1)`,
   `b = next_or(rows)`, `r = next_or(cols)`.
2. `Config::allow_screen_readback == false` -> unhandled, no reply.
3. Half-open screen rectangle, saturating: `top = (t-1+min).clamp(min, max)`, `left = (l-1).min(cols)`,
   `bottom = (b+min).clamp(min, max)`, `right = r.min(cols)`; reversed -> empty.
4. Sum over the **visible screen** rows `top..bottom`, cols `left..right` of every scalar in the
   cell (`Scalar(c)` -> `c as u32`; `Grapheme` -> each scalar of the cluster); blanks, unwritten
   cells and spacers count `0x20`; no attributes, no negation (xterm `checksumExtension 7`).
5. Reply with `checksum & 0xFFFF` as 4 upper-case hex digits. Never reads scrollback.

## DECRQSS (`DCS $ q`)

Request = the payload bytes exactly. Known requests (`value`):

| `req` | `value` |
|---|---|
| `m` | SGR of the pen: `0`, then `;`-joined in order: `1` `2` `3` `5` `6` `7` `8` `9`, the underline (`4`, `4:2`, `4:3`, `4:4`, `4:5`), `53`; fg (`30+i` / `90+i-8` for named 0-15, `38;5;n`, `38;2;r;g;b`; `Foreground` and other named slots omitted); bg (`40`/`100`/`48;..` likewise); underline colour (`58;5;n`, `58;2;r;g;b`; named omitted) |
| `r` | `{region.top + 1};{region.bottom}` |
| `SP q` | DECSCUSR selector of the stored-or-default style (not `cursor_style()`): Block `1`, Underline `3`, Beam `5`, `HollowBlock` and `Hidden` as Block `1`; `+1` when steady |
| `" q` | `0` (DECSCA is not implemented; the pen is never protected) |
| `" p` | `62;1` (DECSCL: the DA1 level, 7-bit) |

Anything else (empty, `s`, `$}`, `*x`, `$|`, `*|`, `mm`, non-ASCII) -> `ESC P 0 $ r ST`.
Fresh terminal: `DCS $ q m ST` -> `ESC P 1 $ r 0 m ST`; `SP q` -> `ESC P 1 $ r 2 SP q ST`.

## XTGETTCAP (`DCS + q`)

- Split the payload on `;`. Names after the 16th (`XTGETTCAP_MAX_NAMES`) and names longer than
  256 hex chars (`XTGETTCAP_MAX_NAME_BYTES = 128` decoded) are dropped; any drop -> **one**
  unhandled for the request. Every other name gets its own reply, in order.
- Decode hex pairs (either case); odd length, non-hex or empty -> unknown. Known:
  `ESC P 1 + r {hex} = {HEX value} ST`; unknown: `ESC P 0 + r {hex} ST`. `{hex}` = the name as
  sent if it is all hex digits, else empty. `{HEX value}` = upper-case hex of the value bytes.
- Table (values are terminfo bytes; `\e` = `1B`, `\a` = `07`). `TN` answers the product name's
  part before `(`, trimmed, when that is non-empty; else `xterm-256color`.

```text
TN xterm-256color | Co 256 | colors 256 | RGB "" | Tc "" | Su ""
setrgbf \e[38:2::%p1%d:%p2%d:%p3%dm | setrgbb \e[48:2::%p1%d:%p2%d:%p3%dm
Ms \e]52;%p1%s;%p2%s\a | Ss \e[%p1%d q | Se \e[2 q | smcup \e[?1049h | rmcup \e[?1049l
bel \a | clear \e[H\e[2J | cr \r | cub1 \b | cud1 \n | cuf1 \e[C | cuu1 \e[A
ed \e[J | el \e[K | home \e[H
```

Example: `DCS + q 544e ST` -> `ESC P 1 + r 544e=787465726D2D323536636F6C6F72 ST`.

## Ceilings

`QUERY_MAX_BYTES = 8 KiB` per DECRQSS/XTGETTCAP payload (dispatch-esc-c0.md): at the ceiling the
request answers nothing and counts once as unhandled; `aborted_dcs` does not move.
