# Threading and synchronized output

`Terminal` is `Send` and `Sync` and takes no lock: you decide who may call what, and when.
Drawing is a two-phase hand-off so the lock is held only while rows are copied.

```text
reader thread            lock                       render thread
-------------            ----                       -------------
feed(bytes, now)  ---->  Terminal  <----  snapshot_update(&mut state, now)   phase 1
                                          palette()
                                    (lock released)
                                          state.map_colors(&palette)         phase 2
                                          draw state.rows()        (state only, no terminal)
```

## Example: both phases

```rust
use std::sync::Mutex;
use std::time::Instant;
use oneterm_vt::{Config, EventBatch, Size, SnapshotState, SnapshotUpdate, Terminal};

let now = Instant::now();
let term = Mutex::new(Terminal::new(Size { rows: 4, cols: 20 }, Config::default()));
let mut state = SnapshotState::new();

// Reader side: feed under the lock, drain the batch, release.
{
    let mut batch = EventBatch::new();
    let mut term = term.lock().unwrap();
    term.feed(b"hello", &mut batch, now);
}

// Render side, phase 1: copy under the lock, take the palette, release.
let (update, palette) = {
    let mut term = term.lock().unwrap();
    (term.snapshot_update(&mut state, now), term.palette())
};

// Phase 2: no terminal needed.
state.map_colors(&palette);
assert_eq!(update, SnapshotUpdate::Full);
assert_eq!(state.rows().len(), 4);
```

`VtEvent::Repaint` is a hint that the feed changed something, not damage. Several feeds may
precede one frame: coalesce on a frame timer (60 Hz), call `snapshot_update` once per frame and let
the per-row stamps tell you what changed. A state holds values only (`Send` and `Sync`), so it may
move to the render thread.

## Synchronized output (`CSI ? 2026 h` / `l`)

A program brackets a redraw with `h` and `l` to keep you from painting half of it. The engine
still **applies every byte at once**; only frames are skipped, so a block that is never closed costs
frames, never memory.

- While the mode is set, `snapshot_update` returns `Unchanged` and copies nothing, for 150 ms after
  the latest `h`; a state that was never built is built anyway (`rows()` must hold the viewport).
- One second after the first `h` the mode is forced off (the watchdog), whatever else arrives.
- Modes, cursor and selection are refreshed during a block; a change seen then is reported once,
  by the next real frame (`Partial { scrolled: 0 }` at least).
- Rows written during the block are all copied by the first frame after it, however many feeds ran.
- `Terminal::sync().is_set()` is what DECRQM `?2026` answers; RIS ends a block, DECSTR keeps it.

```rust
use std::time::{Duration, Instant};
use oneterm_vt::{Config, EventBatch, Size, SnapshotState, SnapshotUpdate, Terminal};

let t0 = Instant::now();
let mut term = Terminal::new(Size { rows: 4, cols: 20 }, Config::default());
let (mut batch, mut state) = (EventBatch::new(), SnapshotState::new());
term.snapshot_update(&mut state, t0);

term.feed(b"\x1b[?2026hhalf a redraw", &mut batch, t0);
let held = term.snapshot_update(&mut state, t0 + Duration::from_millis(10));
assert_eq!(held, SnapshotUpdate::Unchanged);
// Nothing more may arrive, so ask again when the engine says a frame is due.
let wake = term.sync().deadline().expect("a block is open");
assert_eq!(wake, t0 + Duration::from_millis(150));
let frame = term.snapshot_update(&mut state, wake);
assert_eq!(frame, SnapshotUpdate::Partial { scrolled: 0 });
assert_eq!(state.changed(), [0]);
```

## Embedder checklist

- Pass the real `now` to `feed` and `snapshot_update`; the engine reads no clock.
- Hold your lock for `feed`, `snapshot_update` and `palette`; release it before `map_colors` and drawing.
- On `Unchanged` while `term.sync().is_set()`, schedule a wake-up at `term.sync().deadline()` and
  call `snapshot_update` again then, even if no output arrives (an event-driven renderer gets no
  further `Repaint`, so a block that is never closed would otherwise freeze the window).
- Never buffer bytes on your side for `?2026`: the engine has already applied them.
- `snapshot_update` takes `&mut Terminal`: the watchdog may close a block inside it.
- One `SnapshotState` per consumer and per terminal; `invalidate()` it before pointing it elsewhere.
