# Mouse pointer shapes

Programs change the mouse pointer with OSC 22: a hand over a button, resize arrows over a border.
The engine implements the protocol (xterm's single name, plus the stack and the queries of the
kitty pointer-shape protocol) and tells you which shape to show. Drawing it is yours.

## Example: a stack of shapes, a query, and the event to follow

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, PointerShapes, Size, Terminal, VtEvent};

// Say what you can draw and what you show by default; the queries answer from this.
let config = Config {
    pointer_shapes: PointerShapes { default_shape: "text".into(), ..PointerShapes::default() },
    ..Config::default()
};
let mut term = Terminal::new(Size { rows: 24, cols: 80 }, config);
let mut batch = EventBatch::new();
let now = Instant::now();

// The program asks what is supported, then pushes a shape.
term.feed(b"\x1b]22;?pointer,no-such-shape,__default__\x1b\\\x1b]22;>pointer\x1b\\", &mut batch, now);
let VtEvent::Reply(span) = &batch.events()[0] else { panic!("an answer") };
assert_eq!(batch.bytes(*span), b"\x1b]22;1,0,text\x1b\\"); // write it back
let VtEvent::Pointer(name) = &batch.events()[1] else { panic!("a shape") };
assert_eq!(batch.str(*name), "pointer"); // show this one
assert_eq!(term.pointer_shape(), Some("pointer")); // the same, read at any time

// It pops the shape again: the pointer is yours.
term.feed(b"\x1b]22;<\x1b\\", &mut batch, now);
assert_eq!(batch.events()[0], VtEvent::PointerReset);
assert_eq!(term.pointer_shape(), None);
```

## Who does what

| The engine | You |
|---|---|
| parses `OSC 22 ; [= > < ?] name,name,..` and checks every name (`a-z 0-9 _ -`, 64 bytes) | map a name to a pointer of your platform; show your own for a name you cannot draw |
| keeps one stack of shapes per screen (16 levels), switches stacks with the alternate screen, empties both on RIS | nothing: follow the events |
| reports the shape to show: `VtEvent::Pointer(name)` / `VtEvent::PointerReset`, and `Terminal::pointer_shape()` | change the pointer on those events; override it while the user selects text or hovers a link |
| answers `?name` queries from `Config::pointer_shapes` and pushes the `Reply` | declare your shapes in `Config::pointer_shapes`; write the `Reply` to the program |

## The sequences

| Sequence | Effect |
|---|---|
| `OSC 22 ; name` or `; =name` | sets the top of the stack (the only level, if the stack was empty). Of a list, the last name is set |
| `OSC 22 ;` or `; =` | the top level asks for your own pointer: `PointerReset`. The level stays, so a later pop returns to the one below |
| `OSC 22 ; >name,name` | pushes every name; the last is shown. A seventeenth level drops the oldest |
| `OSC 22 ; <` | pops one level; on an empty stack nothing happens and nothing is reported |
| `OSC 22 ; ?name,name` | `Reply` `OSC 22 ; v,v ST` with one value per name, ended as the query ended |

Query values: `__current__` is the shape shown or `0`; `__default__` and `__grabbed__` are
`PointerShapes::default_shape` and `grabbed_shape`; any other name is `1` if it is in
`PointerShapes::supported`, else `0`.

- Every set and push, and every pop that removed a level, reports the shape now shown, even when
  it is the same name again. A switch between the screens reports only when their shapes differ,
  so a program that leaves the alternate screen without tidying up leaves no shape behind.
- A name is not checked against `supported`: xterm's X11 names (`left_ptr`, `hand2`) and names of
  your own arrive as they were sent.
- `PointerShapes::default()` supports the 30 CSS cursor names of the protocol
  (`PointerShapes::CSS`), with `text` as the default shape and `default` (the arrow) while a
  program reports the mouse. Give your real values: programs use them to restore the pointer.

## Ceilings

| Ceiling | Value | Past it | Counter |
|---|---|---|---|
| a shape name | `a-z 0-9 _ -`, 64 bytes | the whole sequence is ignored | `unhandled_sequences` |
| levels of one stack | 16 | the oldest is dropped | - |
| names in one query | 64 | no answer | `unhandled_sequences` |
| names in `Config::pointer_shapes` | same rule as a shape name | dropped, or replaced by the default, when the terminal is built | - |

## What you owe

- Fill `Config::pointer_shapes` with what you really draw; a `1` you cannot honour misleads programs.
- Follow `Pointer` and `PointerReset` (or poll `Terminal::pointer_shape()` when you draw).
- Write every `Reply`, in order.
