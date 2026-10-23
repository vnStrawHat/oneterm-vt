# Quickstart

A `Terminal` turns the bytes a program wrote into a grid, events and counters. Drawing is a
second, separate pull: `snapshot_update` copies the rows that changed into a `SnapshotState`.
You own the transport, the lock and the event loop; the crate owns bytes-to-grid semantics and
holds no lock, thread or clock (you pass `now`).

## Example: feed output, answer events, print the screen

```rust
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, Size, SnapshotContent, SnapshotState, Terminal, VtEvent};

let now = Instant::now();
let mut term = Terminal::new(Size { rows: 4, cols: 20 }, Config::default());
let mut batch = EventBatch::new();
let mut state = SnapshotState::new();

// 1. Feed what the program wrote (any chunking gives the same screen).
term.feed(b"hello \x1b[1mworld\x1b[0m\r\n$ \x1b[c", &mut batch, now);

// 2. Drain events: replies go back to the program, verbatim and in order.
let mut to_program = Vec::new();
for event in batch.iter() {
    if let VtEvent::Reply(span) = event {
        to_program.extend_from_slice(batch.bytes(*span));
    }
}
assert_eq!(to_program, b"\x1b[?62;4;22c"); // the answer to the DA1 query above

// 3. Pull the damage-tracked view of the screen.
term.snapshot_update(&mut state, now);

// 4. Read the rows: one cell per column, wide glyphs followed by a spacer.
let mut lines = Vec::new();
for row in state.rows() {
    let mut line = String::new();
    for cell in &row.cells {
        if cell.width.is_spacer() {
            continue;
        }
        match cell.content {
            SnapshotContent::Scalar(c) => line.push(c),
            SnapshotContent::Cluster { start, len } => line.extend(row.cluster(start, len)),
        }
    }
    lines.push(line.trim_end().to_owned());
}
assert_eq!(lines[0], "hello world");
assert_eq!(lines[1], "$");
```

## The loop

1. Read bytes from the program (a PTY, a pipe, a socket) and call `term.feed(bytes, &mut batch, now)`.
2. Walk `batch.iter()`; write every `Reply` to the program, handle the rest (see [events](events.md)).
3. On `Repaint`, or on a frame timer, call `term.snapshot_update(&mut state, now)`.
4. Draw `state.rows()` (see [snapshot](snapshot.md)); tell the program about size changes with
   `term.resize(size, policy)` (see [resize](resize.md)).

## You owe the engine

- A real `now` on every `feed` and `snapshot_update` (tests pass a synthetic one).
- Every `Reply`, in order, back to the program: programs wait for their answers.
- One `SnapshotState` per consumer, kept across frames; a fresh one per frame copies everything.
- Your own locking: `Terminal` is `Send` and `Sync` and takes no lock (see [threading](threading.md)).

Bad input is never an error. What the engine cannot honour is dropped or degraded and counted in
the `FeedStats` that `feed` returns (see [limits](limits.md)).
