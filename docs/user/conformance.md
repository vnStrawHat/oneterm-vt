# Conformance

What `Terminal` understands, what it deliberately only stores, and what it leaves out. Anything not
listed is counted in `FeedStats::unhandled_sequences` and changes nothing.

## Example: ask what a mode answers

DECRQM tells a program whether a mode is live. A mode the engine only stores for you answers
"set" after `h`; a mode nothing reads never does.

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, Size, Terminal, VtEvent};

fn answer(term: &mut Terminal, query: &[u8]) -> Vec<u8> {
    let mut batch = EventBatch::new();
    term.feed(query, &mut batch, Instant::now());
    let mut out = Vec::new();
    for event in batch.iter() {
        if let VtEvent::Reply(span) = event {
            out.extend_from_slice(batch.bytes(*span));
        }
    }
    out
}

let mut term = Terminal::new(Size { rows: 24, cols: 80 }, Config::default());
assert_eq!(answer(&mut term, b"\x1b[?2004$p"), b"\x1b[?2004;2$y"); // bracketed paste: reset
answer(&mut term, b"\x1b[?2004h");
assert_eq!(answer(&mut term, b"\x1b[?2004$p"), b"\x1b[?2004;1$y"); // set: yours to read
answer(&mut term, b"\x1b[?3h"); // DECCOLM clears the screen but never changes the width
assert_eq!(answer(&mut term, b"\x1b[?3$p"), b"\x1b[?3;0$y"); // inert: not supported
assert_eq!(answer(&mut term, b"\x1b[?9001$p"), b"\x1b[?9001;2$y"); // accepted, answers reset
assert_eq!(answer(&mut term, b"\x1b[?9999$p"), b"\x1b[?9999;0$y"); // unknown
```

## Supported

| Group | Sequences |
|---|---|
| C0 | `BEL BS HT LF VT FF CR SO SI`; `DEL` is ignored; the rest and every C1 byte are unhandled |
| ESC | `IND NEL HTS RI SS2 SS3 LS2 LS3 DECSC DECRC DECALN RIS DECID`, `ESC (` `)` `*` `+` with `B` or `0`, `ESC =` `ESC >` |
| cursor | `CUU CUD CUF CUB CNL CPL CHA HPA HPR VPA VPR CUP HVP CHT CBT`, `SCOSC SCORC`, `DECSTBM` |
| edit | `ICH DCH ECH EL ED IL DL SU SD REP`, `TBC`, `DECST8C` (`CSI ? 5 W`) |
| attributes | `SGR`: attributes, 16/256/RGB colours in `;` and `:` forms, underline styles and colour |
| modes | `SM RM` (ANSI and `?`), `DECSTR`, `DECSCUSR`, `DECRQM`, kitty keyboard flags, `modifyOtherKeys` |
| reports | `DA1 DA2 DA3 DSR CPR DECXCPR XTVERSION DECRQSS DECRQCRA XTGETTCAP`, `CSI 14 t`, `CSI 18 t` |
| window | `CSI 22 t` / `CSI 23 t` title stack |
| OSC | `0 1 2 4 7 8 9 9;4 10 11 12 17 19 22 50 52 104 110 111 112 133`; others routable with `OscRoutes` |
| DCS | `$ q` (DECRQSS), `+ q` (XTGETTCAP); APC, SOS and PM are consumed and counted |

Modes with a live effect: `?1 ?5 ?6 ?7 ?12 ?25 ?45 ?47 ?1047 ?1048 ?1049 ?2027` and `4`, and
`?2026` (synchronized output, see [threading](threading.md)). Modes the engine stores and you read
(`Terminal::mode`, `Terminal::mode_snapshot`): `?9 ?1000 ?1002 ?1003 ?1004 ?1005 ?1006 ?1007 ?1015
?1042 ?2004`. Mouse protocols are exclusive on `h`, as are the three encodings.

Alternate screens: `?47` and `?1047` do not save the cursor, `?1047` clears on leave,
`?1049` saves the primary cursor and clears on entry. `SGR 21` is double underline, not "bold off".

## Inert modes

| Mode | DECRQM | Why |
|---|---|---|
| `?3` DECCOLM | `0` | it clears the screen and homes the cursor, but the width is yours |
| `?1048` | `2` | it saves or restores the cursor and keeps no state |
| `?9001` win32-input, `20` LNM | `2` | accepted silently; a line feed never implies a return |

## DECRQCRA

`CSI Pid ; Pp ; Pt ; Pl ; Pb ; Pr * y` sums the scalars of a rectangle of the visible screen. It
is a screen read-back, so it is off: with `Config::allow_screen_readback == false` it is counted as
unhandled and gets no reply. Origin mode makes the rectangle region-relative; scrollback is never
read.

## Known gaps

- `CSI h` and `CSI ? h` with no parameter count one unhandled sequence.
- Left and right margins (`DECSLRM`, `?69`), `DECSCA`, `DECSED`, `DECSEL`, and `CSI * x` (`DECCARA`
  family) are not implemented: counted, no effect.
- DA1 reports level 62 (VT220 with Sixel and ANSI colour); VT320+ features are not claimed.
- Only 7-bit introducers are sequences. A raw C1 byte (`0x80..=0x9F`) is a control that is counted
  as unhandled, never a CSI or OSC introducer.
- Sixel is answered in DA1 but not decoded yet: `DCS q` is counted as unhandled.
- `CSI 14 t` answers `4;0;0` until the cell size is known.
