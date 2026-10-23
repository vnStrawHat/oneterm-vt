# Parser

`oneterm_vt::parser::Parser` turns a byte stream into calls on your `Dispatch` sink: text, control
bytes, and escape, CSI, OSC, DCS and APC sequences. It has no grid or mode, so it suits tools that
strip, record or sniff sequences. Input is untrusted: it never panics or errors, it truncates and flags.

## Example: strip escapes

Only `print_str` and `execute` are required. The input is split inside a CSI on purpose.

```rust
use oneterm_vt::parser::{Dispatch, Parser};

#[derive(Default)]
struct StripEscapes {
    text: String,
}

impl Dispatch for StripEscapes {
    fn print_str(&mut self, text: &str) {
        self.text.push_str(text);
    }
    fn execute(&mut self, byte: u8) {
        if byte == b'\n' {
            self.text.push('\n');
        }
    }
}

let input = b"\x1b[1;31mred\x1b[0m plain\r\n";
let mut parser = Parser::new();
let mut sink = StripEscapes::default();
parser.advance(&mut sink, &input[..3]);
parser.advance(&mut sink, &input[3..]);
assert_eq!(sink.text, "red plain\n");
```

## Reading SGR parameters

`Params::groups()` yields one slice per `;` parameter; `:` sub-parameters stay in the group.

```rust
use oneterm_vt::parser::{Dispatch, Intermediates, Params, Parser};

#[derive(Default)]
struct Sgr {
    groups: Vec<Vec<u16>>,
}

impl Dispatch for Sgr {
    fn print_str(&mut self, _text: &str) {}
    fn execute(&mut self, _byte: u8) {}
    fn csi(&mut self, params: &Params, _: &Intermediates, ignore: bool, byte: u8) {
        if !ignore && byte == b'm' {
            self.groups = params.groups().map(<[u16]>::to_vec).collect();
        }
    }
}

let mut parser = Parser::new();
let mut sink = Sgr::default();
parser.advance(&mut sink, b"\x1b[38:2::255:0:0m");
assert_eq!(sink.groups, [[38, 2, 0, 255, 0, 0]]);
parser.advance(&mut sink, b"\x1b[38;2;255;0;0m");
assert_eq!(sink.groups, [[38], [2], [255], [0], [0]]);
```

## Reading an OSC title

`code` is `None` when entry 0 is not a number; `params.get(1)` is the first argument.

```rust
use oneterm_vt::parser::{Dispatch, OscParams, Parser, StringTerm};

#[derive(Default)]
struct Titles {
    last: Option<String>,
}

impl Dispatch for Titles {
    fn print_str(&mut self, _text: &str) {}
    fn execute(&mut self, _byte: u8) {}
    fn osc(&mut self, code: Option<u32>, params: &OscParams<'_>, _: StringTerm, _: bool) {
        if code == Some(2) {
            self.last = params.get(1).map(|title| String::from_utf8_lossy(title).into_owned());
        }
    }
}

let mut parser = Parser::new();
let mut sink = Titles::default();
parser.advance(&mut sink, b"\x1b]2;build\x07");
assert_eq!(sink.last.as_deref(), Some("build"));
```

## What the sink owes

- Drop a sequence when `ignore == true` (malformed header or a limit was hit); count it if you care.
- Keep `osc_allows_large` pure: its answer may depend on `code` only. It raises a memory ceiling, it is not a policy.
- A bare `ESC \` (the usual OSC/DCS/APC end) arrives as `esc(.., b'\\')`: treat it as a no-op.
- DCS and APC payloads arrive byte by byte between `dcs_hook`/`dcs_unhook` and `apc_start`/`apc_end`; buffer them yourself.
- OSC slices live only during the call; copy what you keep. `Parser::reset` calls nothing on the sink: reset your own DCS/APC state.
- `print_str` run boundaries carry no meaning; controls and chunk ends split runs.

## Limits

| Constant | Value | Past it |
|---|---:|---|
| `MAX_PARAMS` | 32 | first 32 values kept, `ignore` set |
| `MAX_INTERMEDIATES` | 2 | extra bytes dropped, `ignore` set |
| `MAX_OSC_PARAMS` | 16 | the last entry holds the rest, `;` included |
| `OSC_INLINE` | 2 KiB | OSC payload cut, `truncated` set |
| `OSC_LARGE` | 8 MiB | same cut, for codes `osc_allows_large` claims |
| `DCS_MAX_BYTES` | 16 MiB | DCS/APC puts stop, one `aborted` end |

A parameter value saturates at 65535. C1 bytes (`0x80..=0x9F`) are executed, never introducers. Invalid UTF-8 prints one `U+FFFD` per bad subpart.

## Chunking

Any chunking gives the same calls as feeding the stream whole (given a pure `osc_allows_large`); a split may fall inside a sequence or a UTF-8 character.
