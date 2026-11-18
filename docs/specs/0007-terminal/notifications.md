# Desktop notifications: OSC 99 and OSC 777 (`terminal/notification.rs`, `terminal/osc.rs`)

Added after v0.2.1. Both numbers are in `OscRoutes::BUILTIN`; both end in the existing
`VtEvent::Notification { title, body }`. OSC 9 is unchanged (osc.md).

## OSC 777

`777 ; notify ; title ; body`: `p1 != "notify"` or no `p2` -> unhandled. `title` = `p2` lossy,
`body` = `text(3..)` lossy. Both empty -> unhandled. Any other command -> unhandled.

## OSC 99: `99 ; metadata ; payload`

```rust
pub(super) const PENDING_NOTIFICATIONS_MAX: usize = 8;
pub(super) const NOTIFICATION_TEXT_MAX: usize = 4096;   // title, and body
pub(super) const NOTIFICATION_ID_MAX: usize = 128;
pub(super) struct PendingNotification { id: String, title: TextPart, body: TextPart }
struct TextPart { bytes: Vec<u8>, group: u32, group_len: u8 }   // bytes + the open base64 group
```

`State.notifications: Vec<PendingNotification>`, the one that waited longest first. RIS clears it.

**Metadata** (`p1`, absent = empty): pairs split on `:`; a pair that is not one key byte, `=`,
value is skipped, as is every key not listed here.

| Key | Meaning | Default |
|---|---|---|
| `i` | id: bytes outside `a-z A-Z 0-9 _ - + .` removed; longer than 128 afterwards -> the whole sequence is unhandled | empty |
| `p` | `title`, `body`, `?` (query), `close` / `alive` (about a shown notification), anything else (`icon`, `buttons`, future types) | `title` |
| `d` | `0` = more chunks follow; any other value = done | done |
| `e` | `1` = the payload is base64; any other value = plain | plain |

**Steps**

1. `p=?` -> push `Reply`: `OSC 99 ; i=<id or 0> : p=? ; o=always:p=title,body,? <terminator>`,
   the terminator echoed. Nothing else happens. These bytes are API.
2. `p=close`, `p=alive` -> unhandled; pending notifications untouched; no reply.
3. Else take the pending notification with this id out of the list, or start an empty one.
4. `title` / `body`: add the payload to that part. Plain: params `2..` rejoined with `;`.
   Base64: exactly one payload param, else "not base64". Any other `p`: nothing added, the
   sequence counts as unhandled (once).
5. Not done -> push the notification to the back of the list; a list already at 8 drops its
   front first (not counted). Done -> report it.

**Text parts**

- Plain bytes are appended as they are. Base64 is decoded as a stream: the bits of a group of
  four characters that a chunk ends inside wait in `group` for the next chunk; `=` ends the
  group early (2 characters = 1 byte, 3 = 2 bytes, 1 = none). So both chunk-then-encode (each
  chunk padded) and encode-then-chunk (cut anywhere, last chunk padded or not) decode.
- A byte outside `A-Z a-z 0-9 + /` and `=`: the chunk adds nothing, the part and its open group
  are as before, `malformed_sequences += 1`. `d` is still honoured.
- A part never holds more than 4,096 bytes: the rest of a chunk is dropped and the sequence
  counts as unhandled.
- Reporting: end the open base64 group; drop a character cut at the end; other invalid UTF-8 is
  replaced. No title -> the body is the title and the body is empty. No text at all -> nothing
  is reported and the sequence counts as unhandled (once, also when step 4 already counted).

**Not carried** (deliberate; the query answer says so by leaving them out): `a`, `c`, `f`, `g`,
`n`, `o`, `s`, `t`, `u`, `w`, icons, buttons, close and alive. `o=unfocused|invisible` is not
applied: the engine has no focus state; the embedder may apply its own rule to every
notification.

## Tests (`terminal/notification_tests.rs`, `tests/feed_chunking.rs`)

One sequence; title + body; joined chunks keep `;`; body-only; no text; chunks without an id;
two ids interleaved; a cut inside a character at every offset; base64 whole, chunk-by-chunk,
and one encoding cut at every offset with and without padding; not base64 (also `;`), a refused
chunk keeps the open group; the query (`ST`, `BEL`, no id, no second `;`, id sanitised, id too
long); skipped keys; payloads without an event; `close` / `alive`; both text caps; the pending
cap; RIS; routes `Drop` and `Forward`; OSC 777 forms. The chunking property feeds OSC 99, 777
and 22 pieces at arbitrary cuts.
