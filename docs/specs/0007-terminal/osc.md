# OSC: routes and builtins (`terminal/osc_routes.rs`, `terminal/osc.rs`)

## `OscRoute`, `OscRoutes`

```rust
#[non_exhaustive] #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum OscRoute { Builtin, BuiltinAndForward, Forward, Drop }
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OscRoutes { forward: CodeSet, suppress: CodeSet, large: CodeSet }
struct CodeSet { low: [u64; 32], high: Vec<u32> }   // bitmap 0..2048 + sorted spill
impl OscRoutes {
    pub const BUILTIN: [u32; 20] = [0, 1, 2, 4, 7, 8, 9, 10, 11, 12, 17, 19, 22, 50, 52, 104, 110, 111, 112, 133];
    pub fn new() -> OscRoutes;                                     // = Default
    pub fn has_builtin(code: u32) -> bool;                         // const-built bitmap of BUILTIN
    pub fn route(&mut self, code: u32, route: OscRoute) -> &mut Self;
    pub fn route_all(&mut self, codes: &[u32], route: OscRoute) -> &mut Self; // route() per code
    pub fn overrides(&self) -> impl Iterator<Item = (u32, OscRoute)> + '_;     // diagnostics, below
    pub fn large(&mut self, code: u32, allow: bool) -> &mut Self;  // OSC_INLINE -> OSC_LARGE ceiling
    pub fn get(&self, code: u32) -> OscRoute;
    pub fn allows_large(&self, code: u32) -> bool;
}
```

| Route | Builtin handler | `VtEvent::Osc` | Counted |
|---|---|---|---|
| `Builtin` (default for `BUILTIN`) | runs | no | per arm rules |
| `BuiltinAndForward` | runs, first | yes, after | per arm rules |
| `Forward` | skipped | yes | no |
| `Drop` (default for every other number) | skipped | no | unhandled |

- `route` stores the route the number will **actually** get, so equal behaviour = equal tables:
  `Builtin` -> clear both bits; `BuiltinAndForward` -> forward (+ suppress if no builtin, i.e.
  `Forward`); `Forward` -> forward + suppress; `Drop` -> suppress only for a builtin number.
  `Builtin` or `BuiltinAndForward` asked for a number without a builtin therefore reads back as
  `Drop` / `Forward`: documented, no assertion (no panic on embedder data).
- `get`: `(forward, suppress)` = `(0,0)` -> `Builtin` if builtin else `Drop`; `(1,0)` ->
  `BuiltinAndForward` if builtin else `Forward`; `(1,1)` -> `Forward`; `(0,1)` -> `Drop`.
- `large` is a memory ceiling only, orthogonal to the route; the handler grants it only while the
  route is not `Drop` (dispatch-esc-c0.md), so a dropped number never accumulates 8 MiB.
- `overrides()`: every number whose `get` differs from its default (`Builtin` for a builtin,
  `Drop` otherwise), ascending: bitmap codes `0..2048`, then the sorted spill (deduplicated).
  Allocates; diagnostics only.
- The table is read when the `Terminal` is built and never changes mid-session.

## Dispatch (`osc(code, params, term, truncated)`)

1. `truncated` -> `truncated_osc += 1`.  2. `code == None` -> unhandled.
3. Route as above. Forwarded params (`push_osc_params`): every parser param, the 16th re-split on `;`, so the embedder sees one span per `;`-separated parameter (param 0 = code text). Linear in the payload: at most `OSC_INLINE + 1` spans per OSC, or `OSC_LARGE + 1` for a number the embedder opted into with `large(n)` (limits.md).

`text(1..)` below = params 1.. rejoined with `;` (the parser split them), built in the arena.

| Code | Behaviour (any rejection -> unhandled, no event) |
|---|---|
| `0`, `2` | fewer than 2 params -> reject. `text(1..)` skipping non-UTF-8 params, trimmed: non-empty -> `title = Some`, `Title`; empty -> `title = None`, `TitleReset` |
| `1` | same text rule -> `IconName` (empty allowed); `title` untouched |
| `4` | colors.md |
| `7` | `truncated` or `text(1..)` not UTF-8 -> reject. `file://` prefix: rest = `authority/path` -> `host` = authority verbatim, `path` = `/` + percent-decoded path (lossy UTF-8), a leading `/` before a drive (`/C:`, `/C:/..`, `/C:\..`) removed; no `/` after the authority -> `host` = rest, `path` empty. No prefix: `host` empty, `path` verbatim. Push `Cwd`. No path building, no filesystem |
| `8` | `params.len() <= 2` -> reject. `p1` = link params, URI = `text(2..)` lossy (via `scratch`). Empty URI -> close the link. `id` = value of the first `id=` in `p1` split on `:`, empty = none. `hyperlinks.intern(id, uri)`; `None` -> link dropped (counted by the delta in events.md), text still prints. The pen's extras get `hyperlink` (graphic kept), interned, set on the template |
| `9` | fewer than 2 params -> reject. `p1 == "4"`: `truncated` -> reject; `state = p2`, `pct = min(p3, 100)` (u32 parse, missing = 0): `0` Remove, `1` Set(pct), `2` Error(pct), `3` Indeterminate, `4` Paused(pct), else reject -> `Progress`. Else: exactly one empty param after the code -> reject; `Notification { title: empty, body: text(1..) lossy }` |
| `10`-`12`, `17`, `19` | colors.md |
| `22` | `p1` non-empty UTF-8 -> `Pointer(p1)`; else reject |
| `50` | `p1` = `CursorShape=0|1|2` -> shape Block/Beam/Underline on the stored-or-default style (blink kept); modes.md event rule; else reject |
| `52` | below |
| `104`, `110`, `111`, `112` | colors.md |
| `133` | `p1`: `A` PromptStart, `B` PromptEnd, `C` OutputStart, `D` OutputEnd { exit_code: `p2` as `i32` or `None` }; other -> reject. Template semantic: A `Prompt`, B `Input`, C `Output`, D `None`. Push `ShellMark` |

`pN` = param N. A zero-padded code (`OSC 007`) is the same number (spec 0002).

## OSC 52 (clipboard)

- Fewer than 3 params -> reject. Selection = first byte of `p1` (empty -> `c`): `c` Clipboard,
  `p` Primary, `s` Secondary, else reject.
- `p2 == "?"` -> `ClipboardLoad { selection }`.
- Else `decode_base64(p2)`: canonical base64 only: length a multiple of 4; alphabet
  `A-Z a-z 0-9 + /`; `=` only in the last quantum, as `xx==` or `xxx=`; decoded bytes valid
  UTF-8. Success -> `ClipboardStore { selection, text }` (empty payload = empty text). Failure
  -> `malformed_sequences += 1`, no event, arena rewound.
- No policy: whether a program may read or write the clipboard is the embedder's.

## Title stack (`terminal/modes.rs`, crate `TitleStack`)

```rust
struct TitleStack { title: Option<String>, stack: Vec<Option<String>> }   // TITLE_STACK_MAX = 16
```

- `CSI 22 t` (any further params ignored): push `title.clone()`; at 16 drop the oldest first.
- `CSI 23 t`: pop; `Some(t)` -> `title = t` and push `Title` / `TitleReset`; empty stack -> nothing.
- `Terminal::title()` -> `title.as_deref()`, `title_depth()` -> `stack.len()`. RIS clears both.
