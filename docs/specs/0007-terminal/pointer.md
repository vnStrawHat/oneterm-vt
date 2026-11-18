# Pointer shapes: OSC 22 (`terminal/pointer.rs`)

Added after v0.3.0. Replaces the "one name, one event" rule of osc.md with the whole protocol:
xterm's `OSC 22 ; name` and the kitty pointer-shape additions (a stack per screen, queries).

Split: the engine owns the protocol state (parsing, name checks, the stacks, the screen switch,
the replies); the embedder owns the pointers (which names it draws, its default shapes, the
drawing) and says so in `Config::pointer_shapes`.

```rust
pub(super) const POINTER_STACK_MAX: usize = 16;   // the protocol asks for at least 16
pub(super) const POINTER_NAME_MAX: usize = 64;
pub(super) const POINTER_QUERY_MAX: usize = 64;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PointerShapes {                 // Config::pointer_shapes
    pub supported: Vec<Cow<'static, str>>, // default: PointerShapes::CSS
    pub default_shape: Cow<'static, str>,  // default "text"
    pub grabbed_shape: Cow<'static, str>,  // default "default"
}
impl PointerShapes { pub const CSS: &'static [&'static str]; }   // the 30 names of the protocol's table

pub(super) struct PointerStack { shapes: Vec<String> }      // "" = a level asking for the own pointer
pub(super) struct PointerStacks { active: PointerStack, inactive: PointerStack }
impl Terminal { pub fn pointer_shape(&self) -> Option<&str>; }   // top of the active stack
```

- A shape name: `a-z 0-9 _ -`, 1 to 64 bytes. `Terminal::new` cleans `PointerShapes`: a
  `supported` entry that is no name is dropped; a `default_shape` / `grabbed_shape` that is none
  becomes the type's default. So no configured text can put a control byte into a reply.
- `State.pointer: PointerStacks`; `swap()` on every real screen switch, next to the keyboard
  stacks; RIS replaces it with the default (both stacks empty).

## `OSC 22 ; p1`

`p1` absent or not UTF-8 -> unhandled. First byte `=`, `>`, `<`, `?` selects the operation and the
rest is the name list (split on `,`); any other first byte: operation `=`, all of `p1` is the list.

| Operation | Rule |
|---|---|
| `=` with an empty list | top level := `""` (nothing on an empty stack); push the event |
| `=` | every name must be a shape name, else unhandled and no change. Each name in turn replaces the top (or becomes the only level): the last one stays. The protocol defines one name here; a list follows the push rule "the last is current". Push the event |
| `>` | same check; each name is a new level, a full stack drops index 0 first. Push the event |
| `<` | pop one level; names ignored. An empty stack: nothing, no event, not counted. Else push the event |
| `?` | empty list or more than 64 names -> unhandled, no reply. Else `Reply`: `OSC 22 ; v,v,.. <terminator echoed>`, one `v` per name: `__current__` -> current name or `0`; `__default__` / `__grabbed__` -> the config values; a name in `supported` -> `1`; anything else -> `0`. No state change, no event |

"Push the event": `Pointer(top)` if the top is a non-empty name, else `PointerReset`.

- Screen switch (`enter_alt` / `leave_alt`, after the swap): push the event iff the current
  shapes of the two stacks differ.
- RIS: `PointerReset` iff the active stack had a current shape (modes.md).
- Setting is not checked against `supported`: that list is the embedder's statement for queries;
  a name outside it reaches the embedder, which shows its own pointer for a name it cannot draw.

## Tests (`terminal/pointer_tests.rs`)

Set with and without `=`; reset; a list sets its last name; a name outside `supported`; text
that is no name (and the 64-byte edge, no parameter, not UTF-8); push/pop order and a pop of an
empty stack; set replaces the top; an empty name keeps its level; the 16-level cap; one stack
per screen; equal shapes report nothing on a switch; leaving the alternate screen leaves no
shape; RIS empties both; query values, both terminators, non-names answer `0`; all 30 CSS names;
`__current__`; config values and their cleaning; the query cap. The chunking property feeds
set, reset, push, pop and query pieces.
