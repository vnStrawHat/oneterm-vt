# 0008: Snapshot and synchronized output (phase 7)

**Goal:** a pull-based, damage-tracked hand-off from `Terminal` to a renderer: one `SnapshotState`
per consumer, rows keyed by `RowId`, damage read from per-row `SeqNo` through a per-state
`Watermark`, resolved style runs, palette, mode snapshot, `?2026` with a timeout; bench tier 3.

**Non-goals:** image decoding and placement filling (`SnapshotPlacement` shape lands, stays empty,
phase 10); `encode_key*` (9); search (8); `hotpath` sites (12); drawing, fonts, GPU.

| File | Covers |
|---|---|
| [state.md](state.md) | `SnapshotState`, `Watermark`, `SnapshotUpdate` rules, accessors, counter bumps, `Terminal::snapshot_update` |
| [rows.md](rows.md) | `SnapshotRow`, `SnapshotCell`, `SnapshotContent`, `StyleRun`, row copy, hyperlink cache |
| [palette-modes.md](palette-modes.md) | `Palette`, theme vs OSC overrides, `ModeSnapshot`, `Mouse*` derivation |
| [sync.md](sync.md) | `SyncState`, `?2026`, timeouts, how updates honour it, watermark semantics |
| [bench.md](bench.md) | tier 3 `render`: frames, `FrameCost`, table, JSON |
| [tests.md](tests.md) | unit tests per decision case, allocation steadiness, sync, user chapters |
| [files.md](files.md) | files, edits to `terminal`/`grid`/`vt-bench`, `lib.rs`, docs, commit |

**Public API:** root `SnapshotState, SnapshotRow, SnapshotCell, SnapshotContent, SnapshotCursor,
SnapshotPlacement, SnapshotUpdate, StyleRun, SyncState, Watermark, ModeSnapshot, Palette,
MouseEncoding, MouseProtocol, MouseReporting`; `Terminal::{snapshot_update, sync, mode_snapshot,
mouse_reporting, set_theme_colors}` (+ `palette`, `SyncState::deadline` if D4/D5 are accepted).

## Acceptance criteria

- [ ] Quality gate; `cargo test -p oneterm-vt` with `--no-default-features` and `--features vt-paranoid`; `RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features`
- [ ] `Unchanged` / `Partial` / `Full` chosen correctly per damage case (every row of state.md's table has a test)
- [ ] Steady-state updates grow no buffer; an idle update copies no row (tests.md)
- [ ] `?2026` suppression, refresh deadline and watchdog tests pass with an injected clock
- [ ] `docs/user/{snapshot,threading,quickstart}.md` exist; their Rust blocks run under `cargo test --doc`
- [ ] `vt-bench render|all` work; tier-3 numbers in bench.md "Acceptance notes"
- [ ] Commit: `feat(snapshot): damage-tracked snapshots, palette, mode snapshot and synchronized output` (minor)

## Decisions (settled; detail in the topic files)

1. Pull model: no reset pass, no engine-side damage flag; two states over one terminal never clear each other.
2. Modes, cursor, selection, placements refresh on every update, a suppressed one included; a change hidden by a suppressed frame is reported once by the next real frame (sticky flag).
3. Rows carry values, never ids (style, cluster scalars, hyperlink strings); clusters stay `Vec<char>` though our arena stores `str`.
4. Theme vs OSC: an override wins per slot, a reset returns the slot to the theme; cells resolve only in `map_colors`.
5. `SnapshotPlacement` and `SnapshotCell::graphic` are final now; empty / `None` until phase 10.
6. Internal only: hyperlink cache is an `FxHashMap` (no O(cells x links) scan); a colour change bumps `palette_epoch` only; no integrity walk inside `snapshot_update` (`feed` already ran it); sync deadlines use `checked_add` (no overflow panic).
7. A resize inside an open `?2026` block is still suppressed (self-heals within 150 ms).

## Design choices with alternatives (reviewer rules A = as proposed, B = the simpler alternative)

| # | Proposed (A) | Simpler alternative (B) | Evidence |
|---|---|---|---|
| D1 | grid `seq < watermark` -> `Full` | `debug_assert!` (panics in debug, copies nothing in release) | a state kept across a recreated `Terminal` would show stale rows |
| D2 | `set_theme_colors` bumps `palette_epoch` only when the theme differs | bump on every call | calling it per frame would make every frame `Full` |
| D3 | tier 3: `cells` = cells **copied** per frame, counted outside the timer; median of `RUNS` cycles | `cells` = viewport area (constant), timed; one sample | a constant area says nothing about damage savings, counting inside the timer skews it, one sample is noise (bench.md) |
| D4 | add `Terminal::palette() -> Palette` (theme with overrides applied) | stored theme has no reader | colour queries are events (0007 D13), so nothing else reads the theme; the embedder needs the effective palette |
| D5 | add `SyncState::deadline() -> Option<Instant>` | no way to learn when to ask again | deadlines stay private; an event-driven renderer gets no further `Repaint`, so a `?2026` timeout could freeze it (sync.md) |

## Open items

None. Every candidate question was answered from the `?2026` convention (sync.md), 0007's hooks
or the grid contract (0004); the design choices above are the only points needing a ruling.
