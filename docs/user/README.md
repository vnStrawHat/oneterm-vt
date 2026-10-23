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
| 01 | `quickstart.md` | create a `Terminal`, feed bytes, read a snapshot | 7 | planned |
| 02 | `threading.md` | who owns the lock, when to draw, synchronized output | 7 | planned |
| 03 | `events.md` | `EventBatch` and every `VtEvent`; answering `Reply`, `ColorQuery` (`ColorKey::color_reply`), `ClipboardLoad`; `OscRoutes` | 6 | planned |
| 04 | `input.md` | keys and mouse to bytes; kitty flags, `modifyOtherKeys` | 9 | planned |
| 05 | `search.md` | literal and `regex` scrollback search | 8 | planned |
| 06 | `graphics.md` | Sixel lifecycle, placements, what you draw | 10 | planned |
| 07 | [`resize.md`](resize.md) | `grid::TerminalGrid::resize`, `ResizePolicy` (PTY vs ConPTY), `ResizeOutcome`; what reflow keeps, what dies | 4, 6 | ready |
| 08 | `limits.md` | every ceiling on hostile input and what happens past it; each `FeedStats` counter | 6 | planned |
| 09 | `conformance.md` | supported sequences and modes, inert modes and their DECRQM answers, the DECRQCRA gate, known gaps | 6+ | planned |
| 10 | `pty.md` | the `pty` transport with `polling`; platform differences | 11 | planned |
| 11 | [`versioning.md`](versioning.md) | what to pin, the semver promise, MSRV, features | 0 | written |
| 12 | `performance.md` | what is measured, history and chart | 12 | see [perf/](../perf/README.md) |
| 13 | `api-map.md` | every public type in one table: what it is for | 13 | planned |
| 14 | `parser.md` | `parser::Parser` and `Dispatch` without a grid: strip, record, sniff sequences | 1 | ready |
| 15 | [`cells-and-styles.md`](cells-and-styles.md) | `Cell`, `Style`, resolving ids through `intern::Interner`, `scalar_width` / `cluster_width` | 2 | ready |
| 16 | [`grid.md`](grid.md) | `grid::TerminalGrid` and `Screen`: rows and `RowId`s, scrollback, viewport, reading rows | 3 | ready |
| 17 | `selection.md` | `Selection`, `SelectionKind`, `Side`: ranges, word/line/block expansion, copied text, `Invalidation` before erases | 5 | planned |

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
