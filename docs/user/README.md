# oneterm-vt user guide

For developers and AI agents embedding `oneterm-vt`. Each chapter is short, starts with a
working example, and names exact API items. Every Rust block is compiled by `cargo test --doc`.

## Read this first

1. AI agent? Read [`for-agents.md`](for-agents.md) first: the loop and five common mistakes.
2. Looking for a name? [`api-map.md`](api-map.md) lists every public type, one row each.
3. Every Rust block in this guide compiles against this version; copy them.
4. You own the transport, the lock, the threads; the crate owns bytes-to-grid semantics.
5. `Terminal::feed(bytes, &mut batch, now)` never fails and never panics; drops are counted in `FeedStats`.
6. Write every `VtEvent::Reply` back to the program, in order.
7. Draw from a reused `SnapshotState`, never from the grid under your lock.
8. Turn keys into bytes with `Terminal::encode_key`; it follows the program's modes.
9. `pty` is a default feature; `default-features = false` leaves the engine alone.
10. What is promised across versions: [`versioning.md`](versioning.md).

**Status:** v0.1.0. Every chapter is written; every Rust block compiles in `cargo test --doc`.

## Chapters

| # | Chapter | Covers |
|---|---|---|
| 00 | [`for-agents.md`](for-agents.md) | for AI agents: the feed / answer / snapshot / encode loop in one example, what you owe, the five most common mistakes |
| 01 | [`quickstart.md`](quickstart.md) | create a `Terminal`, feed bytes, drain events, print one snapshot as text |
| 02 | [`threading.md`](threading.md) | who owns the lock, the two-phase hand-off, when to draw, `?2026` and its timeout |
| 03 | [`events.md`](events.md) | `EventBatch` and every `VtEvent`; answering `Reply`, `ColorQuery` (`ColorKey::color_reply`), `ClipboardLoad`; `OscRoutes` |
| 04 | [`input.md`](input.md) | `input::encode_key_event` / `Terminal::encode_key`: the four-rung ladder (legacy, `modifyOtherKeys`, kitty flags), `KeyEvent` fields; `encode_mouse_*` per reporting mode and encoding; what the embedder still owes (paste, focus, alternate scroll) |
| 05 | [`search.md`](search.md) | `search::GridText` and `search_grid_text`: copy under the lock, search without it; matches across wrapped rows, `SearchMatch` to viewport rows, `MAX_MATCHES`; `regex` feature |
| 06 | [`graphics.md`](graphics.md) | Sixel lifecycle: `Terminal::take_graphics` (`GraphicData`), `Placement` / `SnapshotPlacement`, drawing by cells with `graphic_offset`, `VtEvent::GraphicReleased`, `set_cell_pixels` and `VIRTUAL_CELL`, ceilings |
| 07 | [`resize.md`](resize.md) | `grid::TerminalGrid::resize`, `ResizePolicy` (PTY vs ConPTY), `ResizeOutcome`; what reflow keeps, what dies |
| 08 | [`limits.md`](limits.md) | every ceiling on hostile input and what happens past it; each `FeedStats` counter |
| 09 | [`conformance.md`](conformance.md) | supported sequences and modes, inert modes and their DECRQM answers, the DECRQCRA gate, known gaps |
| 10 | [`pty.md`](pty.md) | `pty::PseudoConsole` polled with your own `polling::Poller`: the two tokens, the read/write return values, `OnResize`, what you owe (deregister, drain, `TERM` in `Options::env`); platform differences, drop behaviour, `conpty.dll` beside your executable; turning `pty` off |
| 11 | [`versioning.md`](versioning.md) | what to pin, the semver promise, MSRV, features |
| 12 | [`performance.md`](performance.md) | what is measured and what is not, reproducing a run, history and chart, the `vt-bench grid --check` trip-wire, profiling with `hotpath-profiling`, why the numbers are not a promise |
| 13 | [`api-map.md`](api-map.md) | every public type and trait, one row each, grouped by module, with the chapter that teaches it |
| 14 | [`parser.md`](parser.md) | `parser::Parser` and `Dispatch` without a grid: strip, record, sniff sequences |
| 15 | [`cells-and-styles.md`](cells-and-styles.md) | `Cell`, `Style`, resolving ids through `intern::Interner`, `scalar_width` / `cluster_width` |
| 16 | [`grid.md`](grid.md) | `grid::TerminalGrid` and `Screen`: rows and `RowId`s, scrollback, viewport, reading rows |
| 17 | [`selection.md`](selection.md) | `Selection`, `SelectionKind`, `Side`: ranges, word/line/block expansion, copied text, `Invalidation` before erases |
| 18 | [`snapshot.md`](snapshot.md) | `SnapshotState` and `SnapshotUpdate` (`Full` / `Partial { scrolled }` / `Unchanged`), rows and style runs, `Palette` and theme vs OSC overrides, `ModeSnapshot`, `MouseProtocol` |

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
  stale example fails `cargo test --doc`. `crates/vt/tests/user_guide.rs` checks the wiring.
