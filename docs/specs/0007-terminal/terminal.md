# `Terminal`, `Config`, `State`: `terminal.rs`

## `Config`

```rust
#[derive(Clone, Debug)]
pub struct Config {
    pub scrollback_limit: u32,             // DEFAULT_SCROLLBACK; the grid clamps to SCROLLBACK_MAX
    pub osc_routes: OscRoutes,             // OscRoutes::new(): every builtin runs, nothing forwarded
    pub default_cursor_style: CursorStyle, // steady Block
    pub semantic_escape_chars: String,     // DEFAULT_SEMANTIC_ESCAPE_CHARS (spec 0006)
    pub product_name: Option<Cow<'static, str>>, // None: "oneterm-vt(<CARGO_PKG_VERSION>)"
    pub allow_screen_readback: bool,       // false: DECRQCRA answers nothing (queries.md)
}
impl Default for Config { .. }
```

- Not `#[non_exhaustive]`, so `Config { x, ..Config::default() }` works outside the crate. Its
  rustdoc says: always use the struct-update form; a new field is a minor bump.
- `Terminal::new` stores `product_name` sanitised (queries.md); `config()` returns the stored copy.
  Read once; `set_scrollback_limit` is the only setter.

## Layout

```rust
pub struct Terminal { parser: Parser, state: State }   // manual Debug: size, alt_active, title
struct State {
    grid: TerminalGrid, interner: Interner, config: Config,
    modes: Modes, cursor_style: Option<CursorStyle>, keyboard: KeyboardStacks, modify_other_keys: u8,
    title: TitleStack, colors: ColorOverrides, shifts: Shifts, preceding_char: Option<char>,
    carry: Option<ClusterCarry>, carry_text: String, scratch: String,
    dcs_query: Option<DcsQuery>, dcs_payload: Vec<u8>, selection: Option<Selection>,
    lines_produced: u64, cell_pixels: (u16, u16), stats: FeedStats, dispatched: bool,
}
struct Shifts { active: u8, single: Option<u8>, saved: [(u8, Option<u8>); 2] } // GL set 0..=3
```

`feed` builds `Handler { state: &mut self.state, out: batch }` and runs
`self.parser.advance(&mut handler, bytes)`: one split borrow, no `RefCell`. Private fields;
the child modules of `terminal` reach them. `Terminal` is `Send + Sync` (asserted in a test).

| Field | Meaning |
|---|---|
| `modes`, `cursor_style`, `keyboard`, `modify_other_keys` | modes.md |
| `title`, `colors` | osc.md, colors.md |
| `shifts` | `active` = GL set (SI/SO/LS2/LS3), `single` = pending SS2/SS3, `saved[screen]` = what DECSC saved (index 0 primary, 1 alternate) |
| `preceding_char` | REP source: last printed scalar before charset mapping; survives escapes; RIS clears |
| `carry`, `carry_text`, `scratch` | `?2027` cross-chunk cluster (dispatch-esc-c0.md); `scratch` is reused for joins (OSC 8 URI, `?2027` run) |
| `dcs_query`, `dcs_payload` | open DECRQSS/XTGETTCAP and its payload (<= `QUERY_MAX_BYTES`, `clear()`ed, capacity kept) |
| `lines_produced` | output lines (below); RIS keeps it |
| `cell_pixels` | `(0, 0)`; read by `CSI 14 t`; phase 10 adds the setter |
| `stats`, `dispatched` | this feed's counters; whether any `Dispatch` call happened |

`lines_produced`: +1 per LF/VT/FF/IND/NEL on either screen (scrolling or not); SU and DL add
their `history_rows`. Wraps, ED 2 and screen switches add nothing. Never reset.

## Methods (all `pub`)

| Method | Rule |
|---|---|
| `new(size: Size, config: Config) -> Terminal` | power-on state; size clamped by the grid |
| `feed(&mut self, bytes: &[u8], batch: &mut EventBatch, now: Instant) -> FeedStats` | below; `now` is unread until phase 7 (bind `_now`) |
| `resize(&mut self, size, policy: ResizePolicy) -> ResizeOutcome` | `grid.resize`; drop the `?2027` carry; prune the selection |
| `set_scrollback_limit(&mut self, limit: u32)` | store in `config`; `grid.set_scrollback_limit` (trims at once, re-rings if the ring size changes); prune the selection |
| `scroll_viewport(&mut self, delta: i32)` | active screen's `scroll_viewport` + `sync_anchors`; negative = towards history, positive = towards output, clamped, so `scroll_viewport(i32::MAX)` is scroll-to-bottom (offset 0). Covers viewport scrolling; there is no `grid_mut` |
| `grid() -> &TerminalGrid`, `screen() -> &Screen`, `interner() -> &Interner`, `config() -> &Config` | readers |
| `stats() -> FeedStats` | the last feed's counters |
| `size() -> Size`, `viewport() -> Viewport` | active screen |
| `mode(Mode) -> bool` | modes.md |
| `cursor_style() -> CursorStyle`, `keyboard_flags() -> KeyboardFlags`, `modify_other_keys() -> u8` | modes.md |
| `title() -> Option<&str>`, `title_depth() -> usize` | osc.md |
| `color(ColorKey) -> Option<Rgb>`, `colors() -> &ColorOverrides` | the OSC override layer only (colors.md) |
| `template() -> Cell`, `style() -> Style` | the pen; `style` resolves its style id |
| `active_charset() -> Charset` | `cursor.charsets[shifts.active]` |
| `lines_produced() -> u64` | above |
| `row_text(id: RowId) -> String` | spec 0006 text.md (`Screen::row_text` on `screen_of(id)`) |
| `hit_test(viewport_row: f32, col: f32) -> (Pos, Side)` | spec 0006 invalidation.md |
| `selection_start(pos, side, kind)`, `selection_update(pos, side)`, `selection_range()`, `has_selection()`, `selection_text()`, `selection_clear()`, `select_all()` | spec 0006 invalidation.md wrappers; each passes `config.semantic_escape_chars` and `interner.graphemes` |

## `feed`

1. `batch.clear()`; `stats = FeedStats { bytes: len, .. }`; `dispatched = false`.
2. Empty `bytes`: return `stats` (no stamp, no events).
3. Read the counter baselines (events.md); `grid.begin_batch()`.
4. `parser.advance(&mut Handler { .. }, bytes)`.
5. `grid.sync_anchors()`; release a selection with `Selection::is_dead` (spec 0006; never prune
   on `to_range() == None`).
6. If `interner.needs_grapheme_sweep()`: `grid.sweep_graphemes(&mut interner.graphemes)` (files.md).
7. Add the counter deltas; if `dispatched`, `batch.push_repaint()`.
8. `debug_assert!` tier-3 integrity with the interner (spec 0004 paranoid.md); return `stats`.

## Hooks for later phases (no reshaping of `Terminal`)

| Phase | Adds |
|---|---|
| 7 | `State.sync: SyncState` replaces the `?2026` mode bit (`Mode::SyncUpdate` reads it, `feed` passes `now`); `State.theme: Palette` + `set_theme_colors(&mut self, theme: &Palette)`; `State.generation`, `palette_epoch` bumped at: resize, set_scrollback_limit, DECCOLM, DECALN, screen switch, `?5` change, RIS, any `ColorOverrides` change; `snapshot_update`, `sync`, `mode_snapshot`, `mouse_reporting` |
| 8 | nothing in `State`; search reads `grid()` and `interner()` |
| 9 | `encode_key`, `encode_key_event` read `mode_snapshot()` (modes, `keyboard`, `modify_other_keys` already stored) |
| 10 | `State.graphics`; DCS `q` (no intermediate) arm opens the decoder (today unhandled); `take_graphics`, `placements`, `set_cell_pixels` writes `cell_pixels`; `VtEvent::GraphicReleased`; end-of-feed placement sweep after step 6; RIS resets graphics; extras-table reclamation |
