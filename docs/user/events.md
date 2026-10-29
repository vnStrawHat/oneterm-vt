# Events

`Terminal::feed(bytes, &mut batch, now)` applies output to the grid and appends `VtEvent`s to a
caller-owned `EventBatch`, in byte order. No embedder code runs inside `feed`, so you may hold your
lock across it and drain afterwards. The engine never acts on an event itself: answering, copying,
titling and notifying are yours. Payloads are spans (`StrSpan`, `ByteSpan`, `ParamSpans`) read back
with `batch.str`, `batch.bytes` and `batch.params`; a span is an index into *its* batch, so read it
before the next `feed`.

## Example: a title and an answer

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, Size, Terminal, VtEvent};

let mut term = Terminal::new(Size { rows: 24, cols: 80 }, Config::default());
let mut batch = EventBatch::new();
term.feed(b"\x1b]2;hi\x07\x1b[c", &mut batch, Instant::now());

let (mut title, mut to_program) = (None, Vec::new());
for event in batch.iter() {
    match event {
        VtEvent::Title(span) => title = Some(batch.str(*span).to_owned()),
        VtEvent::Reply(span) => to_program.extend_from_slice(batch.bytes(*span)),
        _ => {}
    }
}
assert_eq!(title.as_deref(), Some("hi"));
assert_eq!(to_program, b"\x1b[?62;4;22c"); // DA1: write it back, verbatim, in order
```

`feed` clears the batch first. `VtEvent` is `#[non_exhaustive]` and not `Clone`: match with a
wildcard arm. `Repaint` comes last, at most once, whenever the feed did anything; it is a hint to
redraw, not damage.

## Every event and what you owe

| Event | Meaning | You |
|---|---|---|
| `Reply(ByteSpan)` | an answer the program asked for | write the bytes to the program, in order |
| `ColorQuery { key, terminator }` | OSC 4/10/11/12/17/19 with `?` | write `key.color_reply(rgb, terminator)` |
| `ClipboardStore { selection, text }` | OSC 52 write, decoded UTF-8 | policy: set the clipboard or refuse |
| `ClipboardLoad { selection }` | OSC 52 `?` | policy: answer with your own OSC 52 or ignore |
| `Title(span)` / `TitleReset` | OSC 0/2, or `CSI 23 t` popping a title | show it / restore your default |
| `IconName(span)` | OSC 1 | optional |
| `Bell` | `BEL` outside a string | beep, flash, or nothing |
| `Cwd { host, path }` | OSC 7, path percent-decoded | use it; no filesystem is touched |
| `Notification { title, body }` | OSC 9 (`title` is empty) | policy: show or drop |
| `Progress(Progress)` | OSC 9;4 | taskbar state |
| `Pointer(span)` | OSC 22 | set the mouse pointer |
| `ShellMark(ShellMark)` | OSC 133 A/B/C/D | prompt navigation |
| `GraphicReleased(GraphicId)` | a Sixel image left the grid (see [graphics](graphics.md)) | free the texture made from that id; ids never return |
| `CursorStyleChanged` | `Terminal::cursor_style()` differs now | redraw the cursor |
| `ScreenCleared` | ED 2, ED 3 or RIS (before their scroll events) | drop caches of the screen |
| `RowsScrolled(..)` / `RowsTrimmed { oldest }` | content moved ids / history rows fell off | move or drop caches keyed on `RowId` |
| `Osc { code, params, terminator, truncated }` | an OSC you routed to yourself | see below |
| `Repaint` | the feed made at least one dispatch call | draw |

## Answering a colour query

The engine keeps only the override layer a program set (`Terminal::color`); the theme underneath is
yours. `ColorKey` is exhaustive: match it without a wildcard, so a new key breaks your build instead of
falling to "no reply". Resolve the colour, then let `ColorKey::color_reply` build the bytes: they are API.

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, Rgb, Size, Terminal, VtEvent};

let mut term = Terminal::new(Size { rows: 4, cols: 20 }, Config::default());
let mut batch = EventBatch::new();
term.feed(b"\x1b]11;?\x07", &mut batch, Instant::now());

let theme_background = Rgb { r: 30, g: 30, b: 30 };
let mut reply = Vec::new();
for event in batch.iter() {
    if let VtEvent::ColorQuery { key, terminator } = event {
        let color = term.color(*key).unwrap_or(theme_background);
        reply = key.color_reply(color, *terminator);
    }
}
assert_eq!(reply, b"\x1b]11;rgb:1e1e/1e1e/1e1e\x07");
```

## OSC routes

`Config::osc_routes` decides per OSC number. The default runs the 20 built-in numbers
(`OscRoutes::BUILTIN`) and drops every other one, counted in `FeedStats::unhandled_sequences`.

| `OscRoute` | Built-in handler | `Osc` event |
|---|---|---|
| `Builtin` (default for a built-in number) | runs | no |
| `BuiltinAndForward` | runs first | yes, after the typed event |
| `Forward` | skipped | yes |
| `Drop` (default for any other number) | skipped | no, counted as unhandled |

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, OscRoute, Size, Terminal, VtEvent};

let mut config = Config::default();
config.osc_routes.route(7501, OscRoute::Forward); // a private number
config.osc_routes.route(2, OscRoute::BuiltinAndForward); // see titles, and also set one
let mut term = Terminal::new(Size { rows: 4, cols: 20 }, config);
let mut batch = EventBatch::new();
term.feed(b"\x1b]2;hi\x07\x1b]7501;a;b\x07", &mut batch, Instant::now());

assert!(matches!(batch.events()[0], VtEvent::Title(_)));
let VtEvent::Osc { code, params, .. } = &batch.events()[2] else { panic!("Osc for 7501") };
assert_eq!(*code, 7501);
let words: Vec<&[u8]> = batch.params(*params).collect();
assert_eq!(words, [&b"7501"[..], b"a", b"b"]); // one span per `;`, parameter 0 is the code
```

A payload over 2 KiB is cut and `truncated` is set; `OscRoutes::large(code, true)` raises that
ceiling to 8 MiB for one number (only while its route is not `Drop`). A truncated OSC 8 opens no
link and closes the open one (it counts as unhandled); call `large(8, true)` to link URIs over 2 KiB.

## Embedder checklist

- Write every `Reply` to the program, in order, before the next input you send it.
- Decide your clipboard, notification and title policy; the engine has none.
- Read spans before the next `feed`; do not keep an event past its batch.
