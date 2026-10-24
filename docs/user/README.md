# oneterm-vt user guide

For developers and AI agents embedding `oneterm-vt`. Each chapter is short, starts with a
working example, and names exact API items. Every Rust block is compiled by `cargo test --doc`.

**Status:** the engine is being built phase by phase ([roadmap](../roadmap.md)). A chapter
appears when its phase lands; until then it is listed as *planned*.

## The crate in five lines

- You own the transport, the lock and the event loop; the crate owns bytes-to-grid semantics.
- `Terminal::feed(bytes, &mut batch, now)` parses output and fills an `EventBatch`.
- You answer events (replies, title, clipboard, bell); the crate never acts on them itself.
- `Terminal::snapshot_update(&mut state, now)` refreshes damage-tracked rows to draw.
- Hostile input never panics and is never an error: it is dropped or degraded, and counted in `FeedStats`.

## Chapters

| # | Chapter | Covers | Phase | Status |
|---|---|---|---|---|
| 00 | `for-agents.md` | for AI agents: the feed / answer / snapshot / encode loop in one example, what you owe, the five most common mistakes | 13 | planned |
| 01 | [`quickstart.md`](quickstart.md) | create a `Terminal`, feed bytes, drain events, print one snapshot as text | 7 | ready |
| 02 | [`threading.md`](threading.md) | who owns the lock, the two-phase hand-off, when to draw, `?2026` and its timeout | 7 | ready |
| 03 | [`events.md`](events.md) | `EventBatch` and every `VtEvent`; answering `Reply`, `ColorQuery` (`ColorKey::color_reply`), `ClipboardLoad`; `OscRoutes` | 6 | ready |
| 04 | [`input.md`](input.md) | `input::encode_key_event` / `Terminal::encode_key`: the four-rung ladder (legacy, `modifyOtherKeys`, kitty flags), `KeyEvent` fields; `encode_mouse_*` per reporting mode and encoding; what the embedder still owes (paste, focus, alternate scroll) | 9 | ready |
| 05 | [`search.md`](search.md) | `search::GridText` and `search_grid_text`: copy under the lock, search without it; matches across wrapped rows, `SearchMatch` to viewport rows, `MAX_MATCHES`; `regex` feature | 8 | ready |
| 06 | [`graphics.md`](graphics.md) | Sixel lifecycle: `Terminal::take_graphics` (`GraphicData`), `Placement` / `SnapshotPlacement`, drawing by cells with `graphic_offset`, `VtEvent::GraphicReleased`, `set_cell_pixels` and `VIRTUAL_CELL`, ceilings | 10 | ready |
| 07 | [`resize.md`](resize.md) | `grid::TerminalGrid::resize`, `ResizePolicy` (PTY vs ConPTY), `ResizeOutcome`; what reflow keeps, what dies | 4, 6 | ready |
| 08 | [`limits.md`](limits.md) | every ceiling on hostile input and what happens past it; each `FeedStats` counter | 6 | ready |
| 09 | [`conformance.md`](conformance.md) | supported sequences and modes, inert modes and their DECRQM answers, the DECRQCRA gate, known gaps | 6+ | ready |
| 10 | [`pty.md`](pty.md) | `pty::PseudoConsole` polled with your own `polling::Poller`: the two tokens, the read/write return values, `OnResize`, what you owe (deregister, drain, `TERM` in `Options::env`); platform differences, drop behaviour, `conpty.dll` beside your executable; turning `pty` off | 11 | ready |
| 11 | [`versioning.md`](versioning.md) | what to pin, the semver promise, MSRV, features | 0 | written |
| 12 | `performance.md` | what is measured and what is not, reproducing a run, history and chart, the `vt-bench grid --check` trip-wire, profiling with `hotpath-profiling`, why the numbers are not a promise | 12 | planned |
| 13 | `api-map.md` | every public type and trait, one line each, grouped by module, with the chapter that teaches it | 13 | planned |
| 14 | `parser.md` | `parser::Parser` and `Dispatch` without a grid: strip, record, sniff sequences | 1 | ready |
| 15 | [`cells-and-styles.md`](cells-and-styles.md) | `Cell`, `Style`, resolving ids through `intern::Interner`, `scalar_width` / `cluster_width` | 2 | ready |
| 16 | [`grid.md`](grid.md) | `grid::TerminalGrid` and `Screen`: rows and `RowId`s, scrollback, viewport, reading rows | 3 | ready |
| 17 | [`selection.md`](selection.md) | `Selection`, `SelectionKind`, `Side`: ranges, word/line/block expansion, copied text, `Invalidation` before erases | 5 | ready |
| 18 | [`snapshot.md`](snapshot.md) | `SnapshotState` and `SnapshotUpdate` (`Full` / `Partial { scrolled }` / `Unchanged`), rows and style runs, `Palette` and theme vs OSC overrides, `ModeSnapshot`, `MouseProtocol` | 7 | ready |

## Features

| Feature | Default | Adds |
|---|---|---|
| `pty` | on | `oneterm_vt::pty` (ConPTY / `openpty`); deps `polling` + `windows-sys` or `libc` |
| `regex` | off | `search::SearchPattern::Regex`; dep `regex` |
| `vt-paranoid` | off | whole-history integrity walk after every mutation; tests and fuzzing only |
| `hotpath-profiling` | off | `hotpath::measure` sites on hot paths; dep `hotpath` |

## Writing rules for chapters

- At most ~120 lines. Example first, then rules as bullets or tables.
- Name items by full path (`oneterm_vt::input::encode_key`); no internal module names.
- State what the embedder owes the engine (calls, replies, ordering) as a checklist.
- Wire the file into `crates/vt/src/lib.rs` under `#[cfg(doctest)]` with `include_str!`, so a
  stale example fails `cargo test --doc`.
