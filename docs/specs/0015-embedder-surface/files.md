# Files to touch

## Source

| File | Change |
|---|---|
| `crates/vt/src/terminal.rs` | `grid_mut`, `interner_mut` next to `grid()` / `interner()` (api.md A1); one sentence on `scroll_viewport` |
| `crates/vt/src/lib.rs` | `pub use intern::{Extras, ExtrasId, GraphicId, Hyperlink, HyperlinkId, Interner};` and one line in the crate doc's module list (A2) |
| `crates/vt/src/terminal/color.rs` | drop `#[non_exhaustive]` on `ColorKey`, add the doc line (A3) |
| `crates/vt/src/parser/mod.rs` | trait signatures (A4); `dispatch_csi` / `hook_dcs` pass `as_slice()`; `Intermediates` leaves the `pub use` |
| `crates/vt/src/parser/state.rs` | `esc` call passes `as_slice()` |
| `crates/vt/src/parser/params.rs` | `Intermediates` -> `pub(crate)`; `as_slice` -> `pub(crate)` if nothing else needs it |
| `crates/vt/src/terminal/dispatch.rs`, `terminal/csi.rs` | `&Intermediates` -> `&[u8]`, drop `.as_slice()` |

## Tests and tools

| File | Change |
|---|---|
| `crates/vt/src/terminal/terminal_tests.rs` | U1-U6 (tests.md) |
| `crates/vt/src/parser/parser_tests.rs` | recording sink to `&[u8]` (P1) |
| `crates/vt/tests/parser_limits.rs` | sink to `&[u8]`; import list drops `Intermediates` |
| `crates/vt/tests/embedder_surface.rs` | new, S1-S6 |
| `crates/vt-bench/src/fixtures.rs` | sinks to `&[u8]` (P2) |
| `fuzz/fuzz_targets/parser.rs` | sink to `&[u8]`; build it once (`cargo +nightly fuzz build parser` on Linux/WSL, or `cargo check` in `fuzz/`) |

## Docs

| File | Change |
|---|---|
| `docs/user/grid.md` | section "Writing cells into a terminal": `begin_batch`, mint ids with `interner_mut`, write with `row_mut(..).set`, `set_wrapped`; a wide cell writes its own spacer, so never write a spacer by hand; doctest |
| `docs/user/parser.md` | SGR example: `&[u8]`, no `Intermediates` import |
| `docs/user/search.md` | "What a match means": doctest that expands one spanning match into `(RowId, Range<usize>)` per row |
| `docs/user/api-map.md` | `Terminal` row names `grid_mut` / `interner_mut`; `intern` rows note the root names; drop `parser::Intermediates` from its row |
| `docs/user/events.md` | one clause: `ColorKey` is exhaustive, match it without a wildcard |
| `crates/vt/public-api.unix.txt`, `public-api.windows.txt` | `scripts/public-api.sh update`; review the diff against api.md |

No new dependency, feature or module. `docs/roadmap.md` is not touched.

## Version and commit

- Snapshot diff has removed/changed lines (`parser::Intermediates`, three `Dispatch` methods,
  `ColorKey` attribute): **major** per `docs/user/versioning.md`, so `0.1.x` -> `0.2.0`.
- One commit: `feat(terminal)!: mutable grid and intern access, root intern names, slice intermediates`
  with body `Spec: docs/specs/0015-embedder-surface` and a `BREAKING CHANGE:` footer naming
  `parser::Dispatch` (`&[u8]` intermediates, `parser::Intermediates` removed) and `ColorKey`
  (now exhaustive).
