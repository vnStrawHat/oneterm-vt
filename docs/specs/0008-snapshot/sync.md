# Synchronized output (`?2026`): `snapshot/sync.rs`

Bytes are **applied** as they arrive; only frames are skipped. Nothing is buffered, so a program
that opens an update and never closes it costs frames, never memory. Both deadlines are checked
against the `now` the caller passes; the engine reads no clock.

## `SyncState`

```rust
pub(crate) const SYNC_REFRESH: Duration = Duration::from_millis(150); // one `h` holds frames this long
pub(crate) const SYNC_WATCHDOG: Duration = Duration::from_secs(1);    // the mode is forced off after this
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SyncState { open_until: Option<Instant>, watchdog: Option<Instant> }
impl SyncState {
    pub fn new() -> SyncState;                            // closed
    pub fn begin(&mut self, now: Instant);                // CSI ? 2026 h
    pub fn end(&mut self);                                // CSI ? 2026 l
    pub fn is_set(&self) -> bool;                         // what DECRQM reports
    pub fn suppresses_frame(&mut self, now: Instant) -> bool;
    pub fn deadline(&self) -> Option<Instant>;            // D5
}
```

| Method | Rule |
|---|---|
| `begin(now)` | `open_until = now + SYNC_REFRESH`; `watchdog` set to `now + SYNC_WATCHDOG` only if `None` (a repeated `h` refreshes the frame deadline, never the watchdog) |
| `end()` | both `None` |
| `is_set()` | `open_until.is_some()`; stays `true` after the refresh deadline passes, until `end` or the watchdog |
| `suppresses_frame(now)` | closed -> `false`; `now >= watchdog` -> `end()`, `false`; else `now < open_until` |
| `deadline()` (**D5**) | closed -> `None`; else `Some(min(open_until, watchdog))`: the instant a suppressed renderer must ask again |

Deadlines use `now.checked_add(..)`, never `+` (which panics on overflow); `None` stores `now`
itself, so an absurd clock value suppresses nothing instead of panicking.

## Terminal wiring (edits to spec 0007)

| Where | Change |
|---|---|
| `State` | `sync: SyncState` replaces the `?2026` bit in `Modes` (remove the bit) |
| `Handler` | gains `now: Instant`; `feed` passes its `now` (no longer `_now`) |
| `?2026 h` / `l` | `sync.begin(now)` / `sync.end()` |
| DECRQM `?2026`, `Terminal::mode(Mode::SyncUpdate)` | `sync.is_set()` -> `1` / `2` (`true` / `false`) |
| RIS | `sync.end()`; DECSTR keeps it |
| `Terminal::sync(&self) -> &SyncState` | read-only view; the watchdog only fires inside `snapshot_update` |

## How `snapshot_update` honours it (state.md step 3)

- `sync.suppresses_frame(now)` is asked once per update, after modes, cursor and selection were
  refreshed (an input encoder reading `state.modes()` stays current inside a block).
- Suppressed and built before -> `Unchanged`, `changed()` empty, rows/size/watermark untouched.
- Never built -> built (`Full`) even inside a block: `rows()` must hold the viewport for every
  result. Suppression starts with the next update.
- A mode, cursor or selection change seen by a suppressed update sets the sticky `meta_dirty`;
  the next real update returns at least `Partial { scrolled: 0 }` and clears it.
- `invalidate()`, a generation bump or a resize inside a block wait for the block too (they are
  read on the next real update; a resize self-heals within `SYNC_REFRESH`).

## Watermark semantics

- The watermark is the grid `seq()` at the last **real** update; a suppressed update never moves it.
- Damage is the per-row stamp, not a list: every row written inside a block has `seq >
  watermark`, so the first real update after the block copies each of them once, however many
  feeds ran. Memory is `O(viewport)`, independent of the bytes received.
- Two states over one terminal share the `SyncState` (suppression is a property of the stream),
  but each keeps its own watermark, so neither can consume the other's damage.

## Embedder checklist (goes to `docs/user/threading.md`)

- Pass a real `now` to `feed` and `snapshot_update`; tests pass a synthetic one.
- On `Unchanged` while `term.sync().is_set()`, schedule a wake-up at `term.sync().deadline()`
  (D5) and call `snapshot_update` again then, even if no output arrives.
- Never buffer bytes on your side for `?2026`: the engine has already applied them.

## Standard

Synchronized output (`CSI ? 2026 h/l`, DECRQM `CSI ? 2026 $ p`), as adopted by contemporary
terminals: the terminal must not render partial updates while set and should time out. The
150 ms / 1 s pair (refresh, watchdog) is fixed by this spec; DECRQM answers `1`/`2` (never `0`/`3`/`4`) since the mode is real.
