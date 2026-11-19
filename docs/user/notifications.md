# Desktop notifications

Programs ask for a desktop notification with OSC 9, OSC 777 or OSC 99 (the kitty protocol: chunks,
icons, buttons, sounds, reports back). The engine does the protocol and hands you one
`VtEvent::Notification`; `EventBatch::notification` reads everything the program said. Showing
it is yours.

## Example: declare, receive, report a click

```rust
use std::time::Instant;
use oneterm_vt::input::NotificationReport;
use oneterm_vt::{
    Config, EventBatch, NotificationSource, NotificationSupport, NotificationUrgency, Size,
    Terminal, VtEvent,
};

// Declare only what you implement: the capability query is answered from this.
let support = NotificationSupport { report: true, buttons: true, urgency: true, ..Default::default() };
let config = Config { notifications: support, ..Config::default() };
let mut term = Terminal::new(Size { rows: 24, cols: 80 }, config);
let mut batch = EventBatch::new();
let now = Instant::now();

term.feed(b"\x1b]99;i=q:p=?;\x1b\\", &mut batch, now);
let VtEvent::Reply(span) = &batch.events()[0] else { panic!("an answer") };
assert_eq!(
    batch.bytes(*span),
    b"\x1b]99;i=q:p=?;a=report:o=always:p=title,body,?,buttons:u=0,1,2\x1b\\"
);

// Three chunks: a title, a base64 body, buttons. Nothing is reported before the last one.
term.feed(b"\x1b]99;i=build:d=0:u=2:a=report;Build failed\x1b\\", &mut batch, now);
term.feed(b"\x1b]99;i=build:d=0:p=body:e=1;MyBlcnJvcnM=\x1b\\", &mut batch, now);
assert!(!batch.iter().any(|event| matches!(event, VtEvent::Notification(_))));
term.feed("\x1b]99;i=build:p=buttons;Open log\u{2028}Dismiss\x1b\\".as_bytes(), &mut batch, now);

let VtEvent::Notification(at) = &batch.events()[0] else { panic!("a notification") };
let notification = batch.notification(*at);
assert_eq!(notification.source, NotificationSource::Osc99);
assert_eq!(batch.str(notification.title), "Build failed");
assert_eq!(batch.str(notification.body), "3 errors");
assert_eq!(batch.strs(notification.buttons).collect::<Vec<_>>(), ["Open log", "Dismiss"]);
assert_eq!(notification.urgency, Some(NotificationUrgency::Critical));
assert!(notification.report_activation);

// The user presses the first button: these are the bytes you write to the program.
let id = batch.str(notification.id);
assert_eq!(NotificationReport::Button { id, button: 1 }.encode(), b"\x1b]99;i=build;1\x1b\\");
```

## Who does what

| The engine | You |
|---|---|
| reads OSC 9, OSC 777 `notify` and OSC 99; joins the chunks of a notification by its id; decodes base64; checks every key and bounds every size | show the notification with your platform's API; treat its text as untrusted |
| answers the query `p=?` from `Config::notifications` | declare in `NotificationSupport` exactly what you implement; write the `Reply` |
| names the sequence it came from in `Notification::source`, and treats the three alike | any policy per source: a quieter OSC 777, a note of the origin on the card |
| hands you every key in `Notification`, whatever you declared | apply `occasion` (only you know your focus), `urgency`, `expiry`, `sound`; replace a shown notification that has the same non-empty `id` |
| passes `icon_names`, `icon_id` and the `icon` bytes (a PNG, JPEG or GIF file, not decoded) | try the names first; decode the data and cache it under `icon_id`; show the cached image when only `icon_id` arrives |
| reports `VtEvent::NotificationClose { id }` and `VtEvent::NotificationAliveQuery { id }` | close that notification; answer with `NotificationReport::Alive` |
| builds report bytes: `input::NotificationReport::encode()`, ids cleaned | write `Activated` / `Button` if `report_activation`, `Closed` (or `CloseUntracked`) if `report_close`; focus your window if `focus_on_activation` |

## What each source fills

| Sequence | `source` | `title` | `body` | Other fields |
|---|---|---|---|---|
| `OSC 9 ; text` (not `9;4`, which is progress) | `Osc9` | empty | the text | as OSC 99 defines a key that was not sent |
| `OSC 777 ; notify ; title ; body` | `Osc777` | the title, may be empty | the rest, `;` kept | the same defaults |
| `OSC 99 ; metadata ; payload` | `Osc99` | never empty (a lone body becomes the title) | may be empty | every key: see `Notification` |

`NotificationSource` is `#[non_exhaustive]`: a `match` on it needs a wildcard arm. Without
`source`, an OSC 99 notification of a title and a body reads exactly as an OSC 777 one.

OSC 99 rules the engine applies:

- Chunks with the same `i` are one notification; `d=0` holds it back until a chunk without it. Keys
  may come with any chunk and a later chunk overrides an earlier one. Chunks without an id join too.
- `e=1` marks base64, padded or not, cut before or after encoding; a chunk may end inside a
  character or a base64 group. Icon data is taken only as base64.
- Buttons are one text split on U+2028; `kinds` and `icon_names` hold each name once.
- A notification without title and body is not reported, whatever else it carries.
- A key the protocol does not define is skipped. A value it does not define is skipped and counted
  in `FeedStats::unhandled_sequences`; the notification is still reported.
- `p=close` without an id does nothing, as the protocol says.

## Ceilings

| Ceiling | Value | Past it | Counter |
|---|---|---|---|
| title, body, button labels | 4,096 bytes each | cut at a character | `unhandled_sequences` |
| icon image | 1 MiB | dropped whole | `unhandled_sequences` |
| id, icon id | 128 characters | the sequence is ignored / the key is skipped | `unhandled_sequences` |
| application name, sound, a kind, an icon name | 256 bytes | the key is skipped | `unhandled_sequences` |
| kinds, icon names | 8 each | further names are skipped | `unhandled_sequences` |
| notifications waiting for their last chunk | 8 | the one that waited longest is dropped | - |
| a payload or name that should be base64 and is not | - | it adds nothing | `malformed_sequences` |

A reset (RIS) forgets the notifications still waiting.

## What you owe

- Declare in `Config::notifications` only what you implement; everything is off by default, which
  answers "titles and bodies, always shown". Programs rely on the answer.
- Write the `Reply` to the query; without it a program concludes OSC 99 is not understood.
- Send a report only when the notification asked for it and you declared the ability.
- Rate-limit: the engine reports every notification. Shells with VTE integration send
  `OSC 777 ; notify` after every command; `source` is how you single those out.
- To turn a source off, drop its number: `config.osc_routes.route(99, OscRoute::Drop)` (also `9`,
  `777`). A dropped OSC 99 answers no query, which is how a program learns it is off.
