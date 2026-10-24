# For AI agents

`oneterm-vt` turns the bytes a program wrote into a grid and events, and keys into the bytes the
program expects. It never does I/O: no process, no socket, no window, no clock, no lock.
The loop below is the whole contract.

## The loop

```rust
use std::time::Instant;
use oneterm_vt::input::{KeyMods, KeySpec, NamedKey};
use oneterm_vt::{Config, EventBatch, Size, SnapshotContent, SnapshotState, Terminal, VtEvent};

let now = Instant::now();
let mut term = Terminal::new(Size { rows: 4, cols: 20 }, Config::default());
let (mut batch, mut state) = (EventBatch::new(), SnapshotState::new());

// 1. Feed what the program wrote: text, application cursor keys (DECCKM), a DA1 query.
term.feed(b"hello\r\n\x1b[?1h\x1b[c", &mut batch, now);

// 2. Answer events: every Reply goes back to the program, verbatim and in order.
let mut to_program = Vec::new();
for event in batch.iter() {
    if let VtEvent::Reply(span) = event {
        to_program.extend_from_slice(batch.bytes(*span));
    }
}
assert_eq!(to_program, b"\x1b[?62;4;22c");

// 3. Pull the screen into one reused state; read row 0 as text.
term.snapshot_update(&mut state, now);
let mut row0 = String::new();
if let Some(row) = state.rows().first() {
    for cell in row.cells.iter().filter(|cell| !cell.width.is_spacer()) {
        match cell.content {
            SnapshotContent::Scalar(c) => row0.push(c),
            SnapshotContent::Cluster { start, len } => row0.extend(row.cluster(start, len)),
        }
    }
}
assert_eq!(row0.trim_end(), "hello");

// 4. Turn a key into program input; DECCKM is on, so ArrowUp is `ESC O A`.
let key = term.encode_key(&KeySpec::Named(NamedKey::ArrowUp), KeyMods::default());
assert_eq!(key, Some(b"\x1bOA".to_vec()));
```

## What you owe

- [ ] Write every `Reply` back to the program, in order.
- [ ] Read spans (`batch.str`, `bytes`, `params`) before the next `feed`.
- [ ] Keep one `SnapshotState` per view, across frames.
- [ ] Resize the terminal and the transport together, with the same `Size`.
- [ ] Get key bytes from `Terminal::encode_key` / `encode_key_event`; send nothing on `None`.
- [ ] Clipboard, title, bell and notifications are your policy; the crate only reports them.

## Five common mistakes

| Mistake | Symptom | Do instead |
|---|---|---|
| dropping `VtEvent::Reply` | programs stall or time out waiting for DA1, DSR, cursor reports | write `batch.bytes(span)` to the program right after `feed`, in order |
| keeping a span past the next `feed` | wrong or empty text; spans index *their* batch | resolve with `batch.str` / `bytes` / `params` first; copy what you keep |
| a new `SnapshotState` per frame | every update is `Full`; damage tracking lost | one state per view; on `Partial { scrolled }` shift, then redraw `changed()` |
| hand-written key bytes (`"\x1b[A"`) | arrows break in full-screen apps and kitty-keyboard apps | `Terminal::encode_key` / `encode_key_event`; send nothing on `None` |
| resizing one side only | the shell wraps at the old width; prompt garbled | `Terminal::resize` and the transport resize with the same `Size` |

## Testing a program headlessly

Feed recorded bytes, then assert on snapshot text, on reply bytes and on the `FeedStats` that
`feed` returns (`unhandled_sequences`, `malformed_sequences`, `truncated_osc`). No window and no
process are needed; the loop above is already such a test.

## Where next

| You want to | Read |
|---|---|
| lock and thread layout, frame timing | [threading](threading.md) |
| every event and what to do with it | [events](events.md) |
| keys, mouse, paste | [input](input.md) |
| draw rows, damage, palette | [snapshot](snapshot.md) |
| run a shell | [pty](pty.md) |
| ceilings on hostile input | [limits](limits.md) |
| find a type by name | [api-map](api-map.md) |
| what a version bump promises | [versioning](versioning.md) |
