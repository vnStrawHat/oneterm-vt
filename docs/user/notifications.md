# Desktop notifications

Programs ask for a desktop notification with OSC 9, OSC 777 or OSC 99. The engine turns all three
into `VtEvent::Notification { title, body }`. Whether, when and how one is shown is yours.

## Example: the OSC 99 query and a notification in two chunks

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, Size, Terminal, VtEvent};

let mut term = Terminal::new(Size { rows: 24, cols: 80 }, Config::default());
let mut batch = EventBatch::new();
let now = Instant::now();

// A program first asks whether OSC 99 is understood. The answer is a `Reply`: write it back.
term.feed(b"\x1b]99;i=app:p=?;\x1b\\", &mut batch, now);
let VtEvent::Reply(span) = &batch.events()[0] else { panic!("an answer") };
assert_eq!(batch.bytes(*span), b"\x1b]99;i=app:p=?;o=always:p=title,body,?\x1b\\");

// A title marked "not done" (`d=0`): nothing is reported yet.
term.feed(b"\x1b]99;i=1:d=0;Build finished\x1b\\", &mut batch, now);
assert!(!batch.iter().any(|event| matches!(event, VtEvent::Notification { .. })));

// The body, base64 (`e=1`), ends the notification.
term.feed(b"\x1b]99;i=1:p=body:e=1;MyB3YXJuaW5ncw==\x1b\\", &mut batch, now);
let VtEvent::Notification { title, body } = &batch.events()[0] else { panic!("a notification") };
assert_eq!((batch.str(*title), batch.str(*body)), ("Build finished", "3 warnings"));
```

## The three sequences

| Sequence | `title` | `body` |
|---|---|---|
| `OSC 9 ; text` (not `9;4`, which is progress) | empty | the text |
| `OSC 777 ; notify ; title ; body` | the title, may be empty | the rest, `;` kept, may be empty |
| `OSC 99 ; metadata ; payload` | never empty | may be empty |

Other `OSC 777` commands (`precmd`, `preexec`) have no handler and are counted as unhandled.

## OSC 99: what is understood

- Payloads `p=title` (the default) and `p=body`. Chunks with the same `i` are joined; `d=0` holds
  the notification back until a chunk without it arrives. A notification with only a body reports
  it as the title; one with no text at all is not reported.
- `e=1`: base64 text, padded or not, cut into chunks before or after it was encoded. A chunk may
  end inside a character or inside a base64 group.
- `p=?`: answered at once with `o=always:p=title,body,?`, ended the way the query ended. The `i`
  of the query is echoed without any character an id may not hold; `i=0` when there was none.
- The answer lists what the event can carry, so a program that asks knows. Not supported, and
  absent from the answer: actions and reports back (`a`, `c`), urgency (`u`), expiry (`w`),
  sounds (`s`), icons (`g`, `n`, `p=icon`), buttons, `p=close` and `p=alive`.
- A key the event has no place for is skipped and the notification is still reported. That
  includes `o=unfocused` and `o=invisible`: the engine does not know your focus, so apply that
  rule yourself if you want it.
- A payload without an event (`icon`, `buttons`, `close`, `alive`) is counted in
  `FeedStats::unhandled_sequences` and changes nothing; the notification it belongs to still ends
  on its last chunk.

## Ceilings

| Ceiling | Value | Past it | Counter |
|---|---|---|---|
| title, and body | 4,096 bytes each | cut at a character | `unhandled_sequences` |
| notifications waiting for their last chunk | 8 | the one that waited longest is dropped | - |
| id | 128 characters | the sequence is ignored, a query gets no answer | `unhandled_sequences` |
| text marked `e=1` that is not base64 | - | the chunk adds nothing | `malformed_sequences` |

A reset (RIS) forgets the notifications still waiting.

## What you owe

- Write the `Reply` to the query, like every other reply. Without it a program concludes that
  OSC 99 is not understood and may send nothing.
- Policy. The engine reports every notification: rate-limit them, skip them while your window
  has the focus if you wish, and treat the text as untrusted. Text sent as base64 may hold
  newlines and other control characters; it is passed on as decoded.
- To turn a protocol off, drop its number: `config.osc_routes.route(99, OscRoute::Drop)` (also
  `9`, `777`). A dropped OSC 99 answers no query, which is how a program learns it is off.
- Shells with VTE integration send `OSC 777 ; notify` after every command; decide whether a
  finished command deserves a notification before showing each one.
