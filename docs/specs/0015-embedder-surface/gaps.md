# Gap table

What a complete embedder (session adapter with a test fixture, a GPU view, a local-shell
transport, replay and benchmark tools) names from the engine: every type, function, method,
field, variant, constant, trait impl and feature, traced through receiver types (`term.grid_mut()
.screen_mut().row_mut(..)`), not only `oneterm_vt::` paths.

Classes: **I** present and identical; **N** present under a different name, path or signature;
**S** present, semantics differ; **M** missing.

## Counts

| Class | Items | Notes |
|---|---|---|
| I | 501 | 137 unit variants, 121 fields, ~110 methods, 44 structs, 27 enums, 4 traits, 4 features, ... |
| N | 15 | 6 root names, `ColorKey` attribute, 3 `Dispatch` methods, 4 `Interner` methods, `Screen::row_mut` |
| S | 4 | `RowMut::set`, `SearchMatch`, `CSI 14 t` via `set_cell_pixels`, SGR `21` via `feed` |
| M | 5 | `Terminal::grid_mut`, `Terminal::interner_mut`, three `ColorKey` variants |
| Total | 525 | distinct items; a method, field or variant counts once however often it is called |

## Identical (I), by area

| Area | Items used as they are |
|---|---|
| Terminal | `new`, `feed`, `resize`, `grid`, `screen`, `interner`, `stats`, `mode`, `mode_snapshot`, `mouse_reporting`, `keyboard_flags`, `cursor_style`, `color`, `colors`, `lines_produced`, `snapshot_update`, `take_graphics`, `hit_test`, `has_selection`, `select_all`, `selection_{start, update, clear, text}` |
| Config / events | `Config` (`scrollback_limit`, `osc_routes`, `product_name`, `allow_screen_readback`), `OscRoute`, `OscRoutes` (`new`, `route`, `get`, `large`, `allows_large`, `overrides`), `EventBatch` (`new`, `clear`, `iter`, `bytes`, `str`, `params`, `push*`), every `VtEvent` variant, `FeedStats` fields, `ShellMark`, `Progress`, `ClipboardKind`, `StringTerm`, `ColorKey::query_prefix`, `ColorOverrides::iter` |
| Cells | `Cell` (`EMPTY`, `with_{content, style, width, extras, semantic}`, `content`, `width`, `style_id`, `extras_id`, `text_char`), `CellContent`, `CellWidth` (+ `is_spacer`), `Style` (`DEFAULT`, fields), `Attrs`, `Color`, `NamedColor`, `Rgb`, `Semantic`, `Interner::{resolve_style, resolve_extras, graphemes, hyperlinks}`, `HyperlinkTable::{intern, resolve}` |
| Grid | `grid::{Screen, RowFlags, Cursor, DEFAULT_SCROLLBACK, SCROLLBACK_MAX}`, `TerminalGrid::{screen_mut, begin_batch, sync_anchors, alt_active}`, `Screen::{rows, cols, row, row_text, history_len, screen_top, visible_top, oldest, newest, scroll_offset, scroll_viewport, scroll_to_bottom, cursor, cursor_row_index}`, `RowRef::{cell, cells, flags, occ, wrapped, is_allocated}`, `RowMut::set_wrapped`, `RowId` (+ `distance`), `Pos`, `Size`, `SeqNo` |
| Snapshot | `SnapshotState` (`new`, `rows`, `cursor`, `modes`, `size`, `selection`, `placements`, `placement`, `hyperlink`, `graphic_offset`, `scroll_offset`, `viewport_top`, `changed`, `invalidate`), `SnapshotUpdate`, `SnapshotRow` (+ `cluster`, `style_of`), `SnapshotCell`, `SnapshotContent`, `SnapshotCursor`, `SnapshotPlacement`, `StyleRun`, `ModeSnapshot`, `MouseProtocol`, `MouseReporting`, `MouseEncoding`, `KeyboardFlags`, `CursorShape`, `CursorStyle`, `GraphicData` |
| Selection, search | `SelectionKind`, `SelectionRange`, `Side`, `ResizePolicy`, `search::{GridText::from_terminal, SearchPattern::Literal, SearchOptions, search_grid_text}`, `SearchMatch::{display_row, grid_line}` |
| Input | `input::{KeyEvent (+ new), KeyEventKind, KeyMods, KeySpec, NamedKey, MouseModifiers, TerminalMouseButton, encode_key, encode_key_event, encode_mouse_press, encode_mouse_release, encode_mouse_move, encode_wheel_event}` |
| Parser | `parser::{Parser (new, advance), Params::groups, OscParams::len, StringTerm}`, `Dispatch` with all eleven methods overridden; only the three below differ |
| PTY | `pty::{PseudoConsole (spawn, child_pid), Options (shell, env, working_directory, glyph_width, escape_args), Shell::new, WindowSize, GlyphWidth::WcsWidth, ChildEvent::Exited, EventedPty, EventedReadWrite, OnResize, PTY_CHILD_EVENT_TOKEN, PTY_READ_WRITE_TOKEN}`; `conpty.dll` beside the executable; launcher identity variables dropped from the child environment |
| Features | `default-features = false`, `pty`, `vt-paranoid`, `hotpath-profiling` |

## Gaps

| # | Embedder expects | Engine today | Class | Decision |
|---|---|---|---|---|
| G1 | `Terminal::grid_mut` | absent | M | **add** (api.md A1) |
| G2 | `Terminal::interner_mut` | absent | M | **add** (A1) |
| G3 | `Extras`, `ExtrasId`, `GraphicId`, `Hyperlink`, `HyperlinkId`, `Interner` at the root | only under `intern::` | N | **add re-exports** (A2) |
| G4 | `ColorKey` matched without a wildcard | `#[non_exhaustive]` | N | **make exhaustive** (A3) |
| G5 | `Dispatch::{esc, csi, dcs_hook}` take `&[u8]` | take `&Intermediates` | N | **pass `&[u8]`** (A4) |
| G6 | `Dispatch::dcs_hook(params, intermediates, byte)` | also takes `ignore: bool` | N | keep; sink adds `_ignore: bool` or drops its no-op override |
| G7 | `ColorKey::{BrightForeground, DimForeground, Dim(u8)}` | absent; slots 259..=268 unused | M | keep absent: no sequence sets or queries them; match arms for them go |
| G8 | `Interner::style(&Style)`, `Interner::extras(&Extras)` | by value (`Copy`) | N | keep; caller drops the `&` |
| G9 | `Interner::grapheme(&[char])`, `resolve_grapheme -> &[char]` | `&str` both ways | N | keep; caller builds or reads a `str` |
| G10 | `Screen::row_mut(id) -> RowMut` | `-> Option<RowMut>`, `None` off the live range | N | keep; caller writes `if let Some(mut row)` |
| G11 | `RowMut::set` stores the cell raw | keeps wide pairs whole: a wide cell writes its spacer, a stray spacer degrades to blank and releases the pair under it | S | keep (the integrity walk relies on it); a fixture writes the wide cell only |
| G12 | `SearchMatch { row, start_col, end_col }`, one row per match | adds `end_row`; a match may span soft-wrapped rows; trailing blanks are not text | S | keep; literals add `end_row`, painters split per row (search.md) |
| G13 | `CSI 14 t` before `set_cell_pixels` answers `4;0;0` | answers with `VIRTUAL_CELL` (`4;480;800` at 24x80) | S | keep (0011 D4) |
| G14 | `SGR 21` cancels bold | double underline | S | keep (0007 decision 7, ECMA-48 / xterm) |

G1-G5 change the engine. G6-G14 change one or a few lines at the caller and are listed so an
embedder knows them before it builds; none of them needs a new engine item.
