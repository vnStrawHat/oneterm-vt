# Roadmap: `oneterm-vt` v0.1.0

Goal: an embeddable terminal core: VT parser, grid with scrollback, reflow, selection,
damage-tracked snapshots, input encoders, Sixel, optional pseudo-console. Built phase by phase
until every item below exists and the perf targets in [perf/README.md](perf/README.md) are met.

## Rules for every phase

- Phase N gets spec `docs/specs/00(N+1)-*`; coder implements; exit = quality gate +
  `cargo test -p oneterm-vt --no-default-features` + advisor review + a release-note-worthy commit subject.
- The phase writes or updates its `docs/user/` chapter; every Rust block in it is a doctest.
- Add a dependency only in the phase that first uses it (`x.workspace = true`).
- `unsafe_code` stays denied except `pty` (phase 11) and the bench allocator (phase 6), each lifted
  per module with a written reason.
- A phase that unblocks a bench tier adds that tier to `vt-bench` and records its numbers in the
  spec's acceptance notes (informational until phase 12).
- Hostile input never panics and never grows memory without a bound; every drop is counted.

## Phases

**0. Scaffold** (spec `0001-scaffold`): workspace, empty `crates/vt`, `crates/vt-bench` (CLI stub +
working `chart`), CI on 3 OS, release flow, LICENSE/NOTICE, docs skeleton. Perf: none.

**1. Parser**: VT500-style state machine; params with `:` sub-params; intermediates; OSC buffer
(inline, then heap up to a cap); DCS passthrough with a byte cap; UTF-8 decode with replacement;
`memchr` scan for the ground-state fast path.
- Public: `parser::{Parser, Dispatch, Params, ParamGroups, ParamSep, Intermediates, OscParams,
  StringTerm, MAX_PARAMS, MAX_INTERMEDIATES, MAX_OSC_PARAMS, OSC_INLINE, OSC_LARGE, DCS_MAX_BYTES}`.
- Accept: proptest "any chunking of the input gives the same dispatch log"; memory stays under
  the documented caps for oversized params/OSC/DCS; 1M random bytes never panic.
- Perf: the 10 bench fixtures + **tier 1 `parser`**.

**2. Cell, intern, width**: packed `Cell`, `Attrs`, colours, `Style`; interning of styles,
grapheme clusters and hyperlinks with a sweep; `scalar_width` and `cluster_width`.
- Public: root `Attrs, Cell, CellContent, CellWidth, Color, NamedColor, Rgb, Semantic, Style,
  scalar_width, cluster_width`; `intern::*`.
- Accept: `size_of::<Cell>() == 8` (const assert); intern round-trip and sweep tests; width
  table test over every scalar value.

**3. Grid**: rows, ring-buffer scrollback, main/alt `Screen`, cursor, tab stops, scroll region,
erase/insert/delete, anchors, stable `RowId` and `SeqNo`. No resize yet.
- Public: `grid::*` except `TerminalGrid::resize`; root `Pos, RowId, RowRef, SeqNo, Size, Viewport`.
- Accept: a scroll with full history allocates nothing; the `vt-paranoid` integrity walk passes
  every grid test.

**4. Reflow and resize**: column reflow over logical lines (wrap flag), row resize, cursor and
anchor tracking, `TerminalGrid::resize`.
- Public: root `ResizeOutcome, ResizePolicy`.
- Accept: proptest "grow then shrink back restores content where the policy promises it".

**5. Selection**: lands before `terminal`, because erase and scroll operations invalidate selections.
- Public: root `Selection, SelectionKind, SelectionRange, Side, Invalidation`.
- Accept: selections survive scroll and reflow; word/line expansion; text extraction.

**6. Events and terminal**: `Terminal`, `Config`; CSI/ESC/OSC/DCS dispatch; modes; charsets; OSC
routing; replies to DA1-3, DSR, DECRQM, XTVERSION, DECRQSS, XTGETTCAP, DECRQCRA and OSC colour
queries; colour overrides; `EventBatch`, `VtEvent`, `FeedStats`; title stack.
- Public: root `Terminal, Config, Mode, ModeState, CursorShape, CursorStyle, KeyboardFlags, ColorKey,
  ColorOverrides, OscRoute, OscRoutes, EventBatch, VtEvent, FeedStats, ByteSpan, StrSpan, ParamSpans,
  ClipboardKind, Progress, ShellMark, StringTerm`.
- Accept: a byte-exact test per reply (these bytes are API, see docs/user/versioning.md, major bump); a test per
  `FeedStats` counter.
- Perf: **tier 2 `grid`** (9 fixtures; `sixel` waits for phase 10), **tier 4 `resize`**,
  **tier 5 `rss`** (counting allocator, `unsafe` lifted in `vt-bench` only).

**7. Snapshot and sync**: damage-tracked `SnapshotState`, style runs, palette, mode snapshot,
synchronized output (`?2026`) with a timeout.
- Public: root `Snapshot*`, `SnapshotUpdate, StyleRun, SyncState, Watermark, ModeSnapshot, Palette,
  MouseEncoding, MouseProtocol, MouseReporting`; `Terminal::{snapshot_update, sync, mode_snapshot}`.
- Accept: `Unchanged` / `Partial` / `Full` chosen correctly per damage case. Perf: **tier 3 `render`**.

**8. Search**: literal scrollback search; `regex` feature.
- Public: `search::{GridText, SearchMatch, SearchOptions, SearchPattern, search_grid_text}`.
- Accept: same literal results with and without `--features regex`; matches across wrapped rows.

**9. Input encoders**: legacy keys, `modifyOtherKeys`, kitty keyboard protocol, mouse (X10,
normal, button, any; default/UTF-8/SGR/urxvt encodings).
- Public: `input::*`; `Terminal::{encode_key, encode_key_event}`.
- Accept: byte-exact table tests per (event, mode snapshot) pair (API, docs/user/versioning.md, minor bump).

**10. Graphics / Sixel**: Sixel decoder, placements, graphic lifetime, `SnapshotPlacement`.
- Public: root `GraphicData, Placement, VIRTUAL_CELL, SnapshotPlacement`;
  `Terminal::{take_graphics, placements}`.
- Accept: decoder caps hold on hostile images; placements scroll and reflow with their rows.
  Perf: tier 2 **`sixel`** (all 10 fixtures live).

**11. PTY** (feature `pty`, default on): ConPTY on Windows, `openpty` on Unix, pollable via `polling`.
- Public: `pty::*` (`SignalMask` Unix only; `PipeReader`, `PipeWriter` Windows only).
- Accept: spawn/read/write/resize/exit contract tests green on 3 OS in CI;
  `cargo tree -p oneterm-vt -e normal --no-default-features` lists exactly
  `bitflags log memchr rustc-hash unicode-segmentation unicode-width` (CI check).

**12. Perf targets**: `hotpath-profiling` sites; `vt-bench grid --check` trip-wire against a
committed baseline.
- Accept: every tier-2 fixture meets its target in [perf/README.md](perf/README.md) on the target
  machine; misses get a profile, a fix, or an owner-approved waiver recorded there.

**13. Release v0.1.0**: user docs complete, public-API snapshot gate, fuzz target, README
examples, then the Release workflow with version `0.1.0`.
- Accept: committed public-API snapshot matches the item lists above; every `docs/user/*.md`
  Rust block compiles; fuzz runs 10 min clean; `perf/history/<platform>/v0.1.0.json` and
  `perf/chart-<platform>.svg` for `linux`, `windows`, `macos` committed; tag `v0.1.0` created by
  the Release workflow.
