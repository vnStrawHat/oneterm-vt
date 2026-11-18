# Desktop notifications: OSC 99 and OSC 777

Files: `terminal/notification.rs` (OSC 99, `NotificationSupport`), `events/notification.rs`
(`Notification` and its enums), `input/notification.rs` (`NotificationReport`), `terminal/osc.rs`
(OSC 9, OSC 777). Both numbers are in `OscRoutes::BUILTIN`.

Split (rewritten after v0.3.0, which carried titles and bodies only): the engine owns the
protocol: every key, the chunks, base64, the bounds, the query answer, the report bytes. The
embedder owns the notification: showing it, focus, sounds, icon lookup and cache, what the user
did. Nothing a program sends is withheld because of what the embedder declared.

## Public surface

```rust
pub enum VtEvent { Notification(NotificationRef),                // OSC 9, 777, 99; batch.notification(at)
                   NotificationClose { id: StrSpan },            // OSC 99 p=close, id never empty
                   NotificationAliveQuery { id: StrSpan }, .. }  // OSC 99 p=alive
#[non_exhaustive] #[derive(PartialEq, Eq, Debug)]
pub struct Notification { pub id, title, body, app_name, icon_id, sound: StrSpan,
    pub kinds, icon_names, buttons: ParamSpans, pub icon: ByteSpan,
    pub urgency: Option<NotificationUrgency>, pub occasion: NotificationOccasion,
    pub expiry: NotificationExpiry,
    pub focus_on_activation, report_activation, report_close: bool }
pub enum NotificationUrgency { Low, Normal, Critical }
pub enum NotificationOccasion { Always, Unfocused, Invisible }      // Default: Always
pub enum NotificationExpiry { Platform, Never, After(u32) }         // Default: Platform; After = ms
impl EventBatch { pub fn notification(&self, at: NotificationRef) -> &Notification;   // never fails
                  pub fn strs(&self, spans: ParamSpans) -> impl Iterator<Item = &str>; }

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct NotificationSupport { pub focus, report, close_events, unfocused, invisible, icons,
    buttons, close, alive, urgency, expiry: bool, pub sounds: Vec<Cow<'static, str>> }   // Config::notifications

#[non_exhaustive] #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum input::NotificationReport<'a> { Activated { id }, Button { id, button: u32 }, Closed { id },
    CloseUntracked { id }, Alive { id, alive: &'a [&'a str] } }     // .encode() -> Vec<u8>
```

OSC 9 and OSC 777 build `Notification::text(title, body)`: every other field as below "default".

The notifications of a feed live in the batch (`EventBatch.notifications`, cleared with it,
shrunk back to 1,024 entries) and the event holds an index: an event stays 32 bytes and a
notification costs no allocation of its own. A boxed notification cost `osc_9_7` 24%.

## OSC 777

`777 ; notify ; title ; body`: `p1 != "notify"` or no `p2` -> unhandled. `title` = `p2` lossy,
`body` = `text(3..)` lossy. Both empty -> unhandled. Any other command -> unhandled. `#[cold]`.

## OSC 99: `99 ; metadata ; payload`

Metadata (`p1`, absent = empty): pairs split on `:`; a pair is one key byte, `=`, value; anything
else is skipped. Keys of the chunk are read first, the rest is applied to the notification.

| Key | Field | Value | Default |
|---|---|---|---|
| `i` | `id` | identifier: bytes outside `a-z A-Z 0-9 _ - + .` removed; over 128 -> whole sequence unhandled | empty |
| `p` | - | `title` `body` `buttons` `icon` `?` `close` `alive`; anything else = unknown type | `title` |
| `d` | - | `0` = more chunks follow; else done | done |
| `e` | - | `1` = payload is base64 | plain |
| `a` | `focus_on_activation`, `report_activation` | comma list of `focus`, `report`, each with an optional leading `-` | focus on, report off |
| `c` | `report_close` | `0` off, else on | off |
| `f` | `app_name` | base64 UTF-8, 256 bytes | empty |
| `t`, `n` | `kinds`, `icon_names` | base64 UTF-8, 256 bytes; repeatable, each name once, 8 names | empty |
| `g` | `icon_id` | identifier, 128 | empty |
| `s` | `sound` | base64 UTF-8, 256 bytes | empty (= `system`) |
| `o` | `occasion` | `always` `unfocused` `invisible` | `Always` |
| `u` | `urgency` | `0` `1` `2` | `None` |
| `w` | `expiry` | `-1` Platform, `0` Never, `n > 0` After(n ms, saturating `u32`) | `Platform` |

A value outside its column: the key is skipped, the sequence counts as unhandled (once); a
base64 value that does not decode to UTF-8: skipped, `malformed_sequences += 1` (once).

**Steps**

1. `p=?` -> `Reply`: `OSC 99 ; i=<id or 0> : p=? ; <answer> <terminator echoed>`. Answer from
   `Config::notifications`, keys in this order, absent when nothing is listed:
   `a=focus,report` `c=1` `o=always[,unfocused][,invisible]`
   `p=title,body,?[,close][,icon][,alive][,buttons]` `s=<sounds>` `u=0,1,2` `w=1`.
   Default config: `o=always:p=title,body,?`. These bytes are API. `Terminal::new` drops sound
   names that are not `a-z A-Z 0-9 _ - .`, 1 to 256 bytes.
2. `p=close`: empty id -> nothing (the protocol's no-op, not counted); else `NotificationClose`.
   `p=alive` -> `NotificationAliveQuery`. Neither touches a pending notification or replies.
3. Else take the pending notification with this id out of `State.notifications`, or start one
   with the defaults; apply the notification keys of this chunk.
4. Payload: `title`, `body`, `buttons` -> that text part (cap 4,096 bytes; plain = params `2..`
   rejoined with `;`, base64 = exactly one param). `icon` -> the icon part, base64 only (plain ->
   unhandled), cap 1 MiB: past it the icon is emptied, marked refused, and its later chunks count
   as unhandled. Unknown type -> unhandled.
5. Not done -> back of the list (a list of 8 drops its front, not counted). Done -> report.

**Parts**: base64 is a stream; the bits of a group cut by a chunk wait for the next; `=` ends a
group (2 chars = 1 byte, 3 = 2). A byte outside the alphabet: the chunk adds nothing, the open
group is as before, `malformed_sequences += 1`; `d` is still honoured.

**Report**: text parts end their group, drop a character cut at the end, replace other invalid
UTF-8. No title -> the body is the title. No text -> nothing is reported, unhandled. Buttons =
the text split on U+2028, empty labels dropped. The icon bytes go into the batch arena as sent.

## Reports (`NotificationReport::encode`, always `ST`)

| Variant | Bytes |
|---|---|
| `Activated { id }` | `OSC 99 ; i=<id> ; ST` |
| `Button { id, button }` | `OSC 99 ; i=<id> ; <button> ST` |
| `Closed { id }` | `OSC 99 ; i=<id>:p=close ; ST` |
| `CloseUntracked { id }` | `OSC 99 ; i=<id>:p=close ; untracked ST` |
| `Alive { id, alive }` | `OSC 99 ; i=<id>:p=alive ; <id,id,..> ST` |

Every id is cleaned as in the table above; an empty id is written `0`; an empty alive id is left out.

## Ceilings

`PENDING_NOTIFICATIONS_MAX` 8, `NOTIFICATION_TEXT_MAX` 4,096, `NOTIFICATION_ID_MAX` 128,
`NOTIFICATION_NAME_MAX` 256, `NOTIFICATION_LIST_MAX` 8, `NOTIFICATION_ICON_MAX` 1 MiB. Worst case
held by pending notifications: 8 x (1 MiB + 3 x 4 KiB + names). RIS clears them.

## Tests

`terminal/notification_tests.rs`: text, chunks, base64 streams, the query, id rules, defaults,
every key, overriding keys, undefined values, name caps, list caps, buttons, icons (bytes, not
base64, the cap), close, alive, the answer per declaration, ceilings, RIS, routes, OSC 777.
`input/notification.rs`: report bytes and id cleaning. `tests/feed_chunking.rs`: OSC 99 pieces
at arbitrary cuts.
