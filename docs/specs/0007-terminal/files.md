# Files to touch

Builds on specs 0002-0006 as implemented; phases 4 (resize) and 5 (selection) land first.

## `crates/vt/src` (new)

| Path | Content | Milestone |
|---|---|---|
| `events.rs` | `VtEvent`, `FeedStats`, `ClipboardKind`, `Progress`, `ShellMark`; `mod batch;` + test decl | 6a |
| `events/batch.rs` | `EventBatch`, `StrSpan`, `ByteSpan`, `ParamSpans`, `EVENT_ARENA_SOFT`, `EVENT_LIST_SOFT` | 6a |
| `terminal.rs` | module doc; `Config`, `Terminal`, `State`, `Shifts`; `feed`, `resize`, accessors, selection wrappers, `hit_test`; child `mod` decls; re-exports | 6a |
| `terminal/dispatch.rs` | `Handler`, `impl Dispatch` (routing), `unhandled`, `report`, C0, ESC, DCS/APC | 6a, 6b |
| `terminal/print.rs` | scalar path, `map_charset`, `?2027` path, `ClusterCarry`, `CLUSTER_CARRY_MAX`, REP | 6a, 6b |
| `terminal/csi.rs` | CSI table, cursor motion, DECSTBM, erase/scroll arms | 6b |
| `terminal/sgr.rs` | SGR parse; `sgr_parameters` (DECRQSS serialisation) | 6b, 6d |
| `terminal/modes.rs` | `Mode`, `ModeState`, `Modes`, mode side effects, alt screens, resets, `CursorShape`, `CursorStyle`, `KeyboardFlags`, `FlagStack`, `KeyboardStacks`, `TitleStack`, `TITLE_STACK_MAX`, `KEYBOARD_STACK_MAX` | 6b |
| `terminal/osc_routes.rs` | `OscRoute`, `OscRoutes`, `CodeSet` | 6c |
| `terminal/osc.rs` | OSC dispatch and every builtin arm, base64, percent decoding | 6c |
| `terminal/color.rs` | `ColorKey`, `ColorOverrides`, `parse_color`, colour arms | 6c |
| `terminal/query.rs` | every reply, product name, `version_number`, DECRQCRA, DECRQSS, XTGETTCAP table, `QUERY_MAX_BYTES`, `XTGETTCAP_MAX_*`, `PRODUCT_NAME_MAX`, `DcsQuery` | 6d |
| `terminal/terminal_tests.rs`, `osc_tests.rs`, `query_tests.rs`; `events/event_tests.rs` | tests.md (`#[cfg(test)] mod ..;` in the owning file) | each |

`tests/terminal_replies.rs`, `tests/feed_stats.rs`, `tests/feed_limits.rs`,
`tests/feed_chunking.rs` (tests.md), each written in the milestone whose behaviour it checks.

## `crates/vt/src` (edits)

| Path | Change |
|---|---|
| `grid/screen/print.rs` | `PrintMode` gains `pub reverse_wrap: bool` (default `false`); `print`, `wrapline` (pub), `put_tab` return `Option<ScrollReport>` (`Some` iff a scroll ran) |
| `grid/screen.rs` | `linefeed`, `reverse_index` return `Option<ScrollReport>`; `backspace(mode: PrintMode)` with the reverse-wrap rule of dispatch-esc-c0.md; crate `designate_charset(slot: u8, Charset)` (slot > 3 ignored), `tab_forward(n)`, `tab_backward(n)` (pending wrap kept), `TabStops::prev_before(col)`; `reset_screen_rows` -> `pub(crate)`; pub `Cursor::template() -> Cell`; crate `set_scrollback_limit(limit, &mut Anchors)` (below) |
| `grid/terminal_grid.rs` | forwards return those reports; `wrapline` returns its report; crate `set_scrollback_limit(limit)` (primary only), `sweep_graphemes(&mut GraphemeArena)`; tier-3 check reachable from `Terminal::feed` |
| `grid/row.rs` | crate `RowMut::remap_graphemes(&GraphemeRemap) -> bool`: rewrite `Grapheme` content ids in place, widths kept; `true` if any cell changed |
| `grid/mod.rs` | delete the module-level `cfg_attr(not(test), expect(dead_code, ..))` in the milestone where it becomes unfulfilled (6b at the latest); never widen it |
| `intern.rs` | crate `InternTable::clear()`: back to only id 0, keeps `exhausted` (RIS clears extras) |
| `selection.rs` | crate `Selection::is_dead(&self, grid) -> bool` (either anchor dead); delete the item-level `expect(dead_code)` on `DEFAULT_SEMANTIC_ESCAPE_CHARS` (6a: `Config::default` reads it) |
| `lib.rs` | below |

**`Screen::set_scrollback_limit`**: clamp to `SCROLLBACK_MAX`; same value -> nothing. Trim
oldest rows while `history_len > limit` (slots to `None`, one kept as `spare`), `offset =
min(offset, history_len)`, `anchors.trim` when anything went. If `(limit + MAX_ROWS)
.next_power_of_two()` differs from `ring_len`, build the new slot array and move each live row to
`id & new_mask` (O(ring), the one re-ring; ids never change). `sync_anchors`, tier-1 check.

**`TerminalGrid::sweep_graphemes(arena)`**: `remap = arena.sweep(ids)` where `ids` iterates the
`Grapheme` ids of every allocated live row flagged `HAS_GRAPHEME` on both screens (no `Vec`);
then `remap_graphemes` on each such row through `row_mut` (stamps the current batch).

`lib.rs` (crate doc gains one sentence on `Terminal`):

```rust
mod events;
mod terminal;
pub use events::{ByteSpan, ClipboardKind, EventBatch, FeedStats, ParamSpans, Progress, ShellMark, StrSpan, VtEvent};
pub use parser::StringTerm;
pub use terminal::{ColorKey, ColorOverrides, Config, CursorShape, CursorStyle, KeyboardFlags, Mode,
                   ModeState, OscRoute, OscRoutes, Terminal};
// + #[cfg(doctest)] includes of docs/user/events.md, limits.md, conformance.md (same pattern)
```

Root list = the roadmap's phase-6 list exactly; nothing else becomes `pub` (helper types,
constants and `DcsQuery` stay crate-private).

## `crates/vt-bench`

| Path | Change |
|---|---|
| `src/throughput.rs` | `measure_tier` fixture filter; `grid_pass` (bench.md) |
| `src/resize.rs` (new) | tier 4 |
| `src/memory.rs` (new) | `CountingAllocator`, `live_heap_bytes`, tier 5; the only `#![allow(unsafe_code, reason = ..)]` |
| `src/perf_history.rs` | `ResizeCost`, `MemoryCost`, `Tiers` fields (bench.md) |
| `src/main.rs` | `#[global_allocator]`; `grid`, `resize`, `rss`, `all` commands; report sections |

## Rules

- Every `pub` item has rustdoc (`#![warn(missing_docs)]`) naming units, clamps and what is
  dropped or counted; no repo paths or spec names in rustdoc.
- No `unwrap`/`expect`/`panic!`/`unreachable!`/indexing that can panic on input; counters
  saturate; `u16` math saturates or clamps first.
- Hot path (`print_str` scalar path): no allocation, no `dyn`; `#[inline]` only with a tier-2
  number recorded in a "why" comment. No `hotpath` attributes, no `log` (no new dependency).
- The quality gate (CLAUDE.md), `--no-default-features` and `--features vt-paranoid` pass after
  **each** milestone 6a-6e; the coder may land them as separate working-tree steps, committed
  together as one phase commit (docs/agents/commits.md).

## Docs

| Action | Path |
|---|---|
| create (coder) | `docs/user/events.md`, `docs/user/limits.md`, `docs/user/conformance.md` (tests.md outlines) |
| edit (coder) | `docs/user/resize.md` gains `Terminal::resize` (row 07 of `docs/user/README.md`) |
| edit (coder, 6e) | bench.md "Acceptance notes" |
| done by architect | `docs/README.md` spec row; `docs/user/README.md` rows 03, 08, 09 |

## Commit

No CHANGELOG. Subject (minor): `feat(terminal): Terminal, VT dispatch, events, OSC routes and replies`
