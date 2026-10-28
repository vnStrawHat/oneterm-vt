# Tests

Unit tests sit in the existing `*_tests.rs` sibling of each module; the surface test is a new
integration test. Each test checks one behaviour.

## Unit (`terminal/terminal_tests.rs`)

| # | Name | Checks |
|---|---|---|
| U1 | `grid_mut_write_is_read_back_by_screen` | `row_mut(screen_top + 1).set(2, cell)` then `screen().row(..).cell(2)` is that cell |
| U2 | `grid_mut_write_needs_a_new_batch_for_the_snapshot` | after one `snapshot_update`, a write without `begin_batch` is not copied; after `grid_mut().begin_batch()` and the write, the next update copies the row and shows the cell |
| U3 | `interner_mut_ids_resolve_in_the_snapshot` | a cell built from `interner_mut().style(..)`, `.grapheme("e\u{301}")`, `.hyperlinks.intern(None, uri)` + `.extras(..)` reads back through the snapshot with that style, cluster text and hyperlink |
| U4 | `row_mut_outside_the_live_range_is_none` | `grid_mut().screen_mut().row_mut(newest + 1)` is `None`; nothing panics |
| U5 | `hand_scrolled_viewport_survives_a_reflow_after_sync_anchors` | 10 numbered lines of history; `screen_mut().scroll_viewport(-3)` + `sync_anchors()`, then a column `resize` that reflows: the viewport top shows the same line it showed before; same result as `Terminal::scroll_viewport(-3)` on a twin terminal |
| U6 | `scroll_to_bottom_through_grid_mut_matches_scroll_viewport_max` | `screen_mut().scroll_to_bottom()` + `sync_anchors()` gives the same `viewport()` as `scroll_viewport(i32::MAX)` |

## Unit (`parser/parser_tests.rs`, `terminal` tests)

| # | Name | Checks |
|---|---|---|
| P1 | existing recording sink (`parser_tests.rs`) | `esc`/`csi`/`dcs_hook` take `&[u8]` and drop `.as_slice()`; every existing expectation (`escape_intermediates`, `escape_third_intermediate_sets_ignore`, the DCS cases) unchanged |
| P2 | `vt-bench` fixture sinks (`crates/vt-bench/src/fixtures.rs`) | same signature update; bench output unchanged |

No reply-byte test changes: DECRQSS, XTGETTCAP, SCS and DECSCUSR answers in
`tests/terminal_replies.rs` stay byte-identical.

## Integration: `crates/vt/tests/embedder_surface.rs` (new)

Uses only `oneterm_vt::` public paths, no `intern::`-only names where a root name exists.

| # | Checks |
|---|---|
| S1 | Root names compile: `oneterm_vt::{Extras, ExtrasId, GraphicId, Hyperlink, HyperlinkId, Interner}`; `ExtrasId::NONE`; `VtEvent::GraphicReleased(GraphicId(1))` constructs |
| S2 | Signature pins as fn pointers: `Terminal::grid_mut: fn(&mut Terminal) -> &mut TerminalGrid`, `Terminal::interner_mut: fn(&mut Terminal) -> &mut Interner` |
| S3 | A fixture-shaped writer: a wide cell, a grapheme cluster, a hyperlinked cell and a wrap flag written through `grid_mut`/`interner_mut` after `begin_batch`; read back through `SnapshotState` (text, width, link id resolves to its URI via `interner().hyperlinks.resolve`) |
| S4 | `ColorKey` matched exhaustively with no wildcard arm (compiles = passes) |
| S5 | A `Dispatch` sink implementing `esc`, `csi`, `dcs_hook` with `&[u8]` and matching `intermediates.first()` |
| S6 | Viewport by hand: `grid_mut().screen_mut().scroll_viewport(-2)`, `grid_mut().sync_anchors()`; `viewport()` moved by 2 |

## Docs (doctests)

- `docs/user/grid.md`: new section "Writing cells into a terminal" (the S3 pattern, short).
- `docs/user/parser.md`: the SGR example drops `Intermediates` and uses `&[u8]`.
- `docs/user/search.md`: one example that turns a spanning match into per-row column ranges
  (first row from `start_col`, middle rows whole, last row to `end_col`).

## Gate

Quality gate (CLAUDE.md), plus `cargo test -p oneterm-vt --no-default-features`,
`RUSTDOCFLAGS=-D warnings cargo doc --no-deps --all-features`, and
`scripts/public-api.sh update` then `check` green.
