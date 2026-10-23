# Mouse (`input/mouse.rs`)

Reads only `ModeSnapshot::mouse: Option<MouseProtocol>` (spec 0008: `reporting` from `?9`/`?1000`/
`?1002`/`?1003`, `encoding` from `?1005`/`?1006`/`?1015`). `None` -> every encoder returns empty.
`row`, `col` are zero-based; reports use `x = col + 1`, `y = row + 1` (saturating).

## Which events are reported

| `reporting` | press | release | move, button held | move, no button (hover) | wheel |
|---|---|---|---|---|---|
| `X10` (`?9`) | yes, codes 0-2 only, no modifiers | empty | empty | empty | empty |
| `Normal` (`?1000`) | yes | yes | empty (**D4**; B: sent) | empty (**D4**; B: sent) | yes |
| `ButtonEvent` (`?1002`) | yes | yes | yes | empty (**D4**; B: sent) | yes |
| `AnyEvent` (`?1003`) | yes | yes | yes | yes | yes |

The engine does not track held buttons: "button held" means `encode_mouse_move(.., Some(b), ..)`.
Wheel: `delta_y > 0` -> up, `< 0` -> down, `0.0` or NaN -> empty (**D5**; B: down).

## Event code `Cb` (before encoding)

| Event | Base | Notes |
|---|---|---|
| press Left / Middle / Right | 0 / 1 / 2 | |
| release | SGR: the button's code; Default, UTF-8, urxvt: 3 | SGR marks release with final `m` |
| move with button `b` | `b + 32` | |
| hover | `3 + 32 = 35` | |
| wheel up / down | 64 / 65 | |
| modifiers (not under `X10`) | `+4` Shift, `+8` Alt, `+16` Ctrl | max `Cb` = 65 + 28 = 93 |

## Encodings (`reporting` is not `X10`)

| `encoding` | Bytes | Coordinate ceiling |
|---|---|---|
| `Default` | `\x1b[M`, then raw bytes `Cb + 32`, `min(x + 32, 255)`, `min(y + 32, 255)` | 223: larger values clamp to 223 (byte `0xff`) |
| `Utf8` | `\x1b[M`, raw byte `Cb + 32`, then `min(x + 32, 0x7ff)` and `min(y + 32, 0x7ff)` as UTF-8 scalars (1 byte below 128, else 2) | 2015: larger values clamp to 2015 (`\xdf\xbf`) |
| `Urxvt` | `\x1b[{Cb + 32};{x};{y}M` (decimal, release = 35, no `m`) | none (`usize` decimal) |
| `Sgr` | `\x1b[<{Cb};{x};{y}M`, release `...m` (decimal, no `+ 32`) | none |

`X10` reporting ignores `encoding` and modifiers: `\x1b[M`, `Cb + 32`, then the `Default`
coordinate bytes. Past the ceiling, xterm sends `0x00` and some terminals drop the event;
this crate clamps (Decision 6), so a click past column 223 reports 223 and is never lost.

## Byte table (every row a test; `N` = Normal, `A` = AnyEvent; coordinates zero-based input)

| Call | Snapshot | Bytes |
|---|---|---|
| press Left (0, 0) | X10 | `\x1b[M\x20\x21\x21` |
| press Left (0, 0) + Ctrl | X10 | `\x1b[M\x20\x21\x21` |
| release / move / move Left / wheel up (0, 0) | X10 | empty |
| every encoder | `mouse: None` | empty |
| press Left (0, 0) | N Sgr | `\x1b[<0;1;1M` |
| release Left (row 2, col 3) | N Sgr | `\x1b[<0;4;3m` |
| press Left + Ctrl (0, 0) | N Sgr | `\x1b[<16;1;1M` |
| release Right + Shift + Ctrl (3, 7) | N Sgr | `\x1b[<22;8;4m` |
| wheel +1.0 / -1.0 (5, 5) | N Sgr | `\x1b[<64;6;6M` / `\x1b[<65;6;6M` |
| wheel 0.0 / NaN (5, 5) | N Sgr | empty (D5) |
| hover (0, 0) | A Sgr | `\x1b[<35;1;1M` |
| press Left (row 500, col 1000) | N Sgr | `\x1b[<0;1001;501M` |
| press Left (200, 200) / (299, 299) | N Sgr | `\x1b[<0;201;201M` / `\x1b[<0;300;300M` |
| press Left (0, 0) | N Default | `\x1b[M\x20\x21\x21` |
| release Left (0, 0) | N Default | `\x1b[M\x23\x21\x21` |
| press Left + Ctrl (0, 0) | N Default | `\x1b[M0!!` |
| press Right + Shift + Alt (row 5, col 10) | N Default | `\x1b[M.+&` |
| press Left (94, 94) / (200, 200) / (500, 500) | N Default | `\x1b[M\x20\x7f\x7f` / `\x1b[M\x20\xe9\xe9` / `\x1b[M\x20\xff\xff` |
| press Left (`usize::MAX`, `usize::MAX`) | N Default | `\x1b[M\x20\xff\xff` (saturating, no panic) |
| wheel +1.0 (5, 5) | N Default | `` \x1b[M`&& `` |
| hover (row 2, col 3) | A Default | `\x1b[MC$#` |
| press Left (0, 0) / (200, 200) | N Utf8 | `\x1b[M\x20\x21\x21` / `\x1b[M\x20\xc3\xa9\xc3\xa9` |
| press Left (299, 299) / (5000, 5000) | N Utf8 | `\x1b[M\x20\xc5\x8c\xc5\x8c` / `\x1b[M\x20\xdf\xbf\xdf\xbf` |
| press Left (299, 299) | N Urxvt | `\x1b[32;300;300M` (every byte < 0x80) |
| release Left (row 2, col 3) | N Urxvt | `\x1b[35;4;3M` |
| press Right + Shift (0, 0) | N Urxvt | `\x1b[38;1;1M` |
| hover (row 2, col 3) | A Urxvt | `\x1b[67;4;3M` |
| move Left (row 2, col 3) | N / ButtonEvent / A, Sgr | empty (D4) / `\x1b[<32;4;3M` / `\x1b[<32;4;3M` |
| hover (row 2, col 3) | N / ButtonEvent, Sgr | empty / empty (D4) |
| press Left (0, 0) | X10 + `encoding: Sgr` | `\x1b[M\x20\x21\x21` |

`(299, 299)` in UTF-8: `300 + 32 = 332 = U+014C` -> `\xc5\x8c`. Hover rows use `AnyEvent` because
D4 empties them under `Normal`; with D4 ruled B they use `Normal`.

## Not encoded

Focus in/out (`?1004`, `CSI I` / `CSI O`): no encoder here and `ModeSnapshot` has no
field; `Terminal::mode(Mode::FocusInOut)` tells the embedder whether to send them itself.
