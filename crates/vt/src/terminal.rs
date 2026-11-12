//! The terminal: bytes in, grid changes, events and counters out.
//!
//! [`Terminal::feed`] runs the parser over output, applies every sequence to the grid and the
//! mode state, and appends [`VtEvent`](crate::VtEvent)s to a caller-owned
//! [`EventBatch`](crate::EventBatch). Nothing here runs embedder code, performs I/O or takes a
//! lock; answers to queries are events the embedder writes back. Input is untrusted: what the
//! engine cannot honour is dropped and counted in [`FeedStats`](crate::FeedStats).

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use rustc_hash::FxHashSet;

use crate::cell::{Cell, Rgb, Semantic, Style};
use crate::events::{EventBatch, FeedStats};
use crate::graphics::{GraphicData, GraphicsState, Placement};
use crate::grid::terminal_grid::{ResizeOutcome, ResizePolicy};
use crate::grid::{
    Charset, DEFAULT_SCROLLBACK, Pos, PrintMode, RowId, Screen, Size, TerminalGrid, Viewport,
};
use crate::input::{self, KeyEvent, KeyMods, KeySpec};
use crate::intern::Interner;
use crate::parser::{Parser, StringTerm};
use crate::selection::{
    DEFAULT_SEMANTIC_ESCAPE_CHARS, Invalidation, Selection, SelectionKind, SelectionRange, Side,
};
use crate::snapshot::{
    EngineView, ModeSnapshot, MouseProtocol, Palette, SnapshotState, SnapshotUpdate, SyncState,
};

mod color;
mod csi;
mod dispatch;
mod graphics;
#[cfg(feature = "kitty-graphics")]
mod kitty;
mod modes;
mod osc;
mod osc_routes;
mod print;
mod query;
mod sgr;

#[cfg(test)]
mod osc_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod terminal_tests;

pub use color::{ColorKey, ColorOverrides};
pub use modes::{CursorShape, CursorStyle, KeyboardFlags, Mode, ModeState};
pub use osc_routes::{OscRoute, OscRoutes};

use dispatch::Handler;
use modes::{KeyboardStacks, Modes, TitleStack};
use print::ClusterCarry;
use query::DcsQuery;

/// How a [`Terminal`] is built. Always use the struct-update form,
/// `Config { scrollback_limit: 500, ..Config::default() }`: a new field is a minor version
/// bump, and that form keeps compiling. A full struct literal may stop compiling on a minor bump.
#[derive(Clone, Debug)]
pub struct Config {
    /// History rows the primary screen keeps (default 10,000); the grid clamps it to
    /// 1,000,000.
    pub scrollback_limit: u32,
    /// Which OSC numbers run built in, are forwarded as events, or are dropped.
    pub osc_routes: OscRoutes,
    /// The cursor style until a program sets one (default: steady block).
    pub default_cursor_style: CursorStyle,
    /// Word delimiters for semantic selections.
    pub semantic_escape_chars: String,
    /// The identity XTVERSION reports. `None` reports `oneterm-vt(<version>)`. Control
    /// characters are dropped and the name is cut at 64 bytes when the terminal is built;
    /// [`Terminal::config`] returns the cleaned copy.
    pub product_name: Option<Cow<'static, str>>,
    /// Whether DECRQCRA may answer with a checksum of the screen. Off, the query is counted
    /// as unhandled: a program must not read the screen back unless the embedder allows it.
    pub allow_screen_readback: bool,
    /// Whether East Asian Ambiguous scalars (`§`, `±`, box drawing, Greek and Cyrillic
    /// letters) take two columns, as CJK legacy fonts draw them. Off (the default), they take
    /// one, which is what programs assume unless the locale says otherwise. Set it to match
    /// your font; the program must agree (most read it from the locale or a setting).
    pub ambiguous_wide: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            scrollback_limit: DEFAULT_SCROLLBACK,
            osc_routes: OscRoutes::new(),
            default_cursor_style: CursorStyle::default(),
            semantic_escape_chars: DEFAULT_SEMANTIC_ESCAPE_CHARS.to_owned(),
            product_name: None,
            allow_screen_readback: false,
            ambiguous_wide: false,
        }
    }
}

/// A terminal emulator core: parser, two screens with scrollback, modes, colour overrides,
/// titles and a selection.
///
/// Feed it output with [`feed`](Self::feed); read the screen through [`grid`](Self::grid) and
/// [`screen`](Self::screen). It is `Send` and `Sync` and holds no lock: the embedder decides
/// who may call what, when.
pub struct Terminal {
    parser: Parser,
    state: State,
}

/// Everything except the parser, so a dispatch handler can borrow it whole.
struct State {
    grid: TerminalGrid,
    interner: Interner,
    config: Config,
    modes: Modes,
    /// `None`: `config.default_cursor_style`.
    cursor_style: Option<CursorStyle>,
    keyboard: KeyboardStacks,
    modify_other_keys: u8,
    title: TitleStack,
    colors: ColorOverrides,
    /// The embedder's colours underneath the program's overrides.
    theme: Palette,
    sync: SyncState,
    /// Bumped whenever rows change outside the per-row change stamps: snapshots rebuild.
    generation: u32,
    /// Bumped whenever an effective colour changes: snapshots rebuild and remap.
    palette_epoch: u32,
    shifts: Shifts,
    /// The last printed scalar before charset mapping, for REP.
    preceding_char: Option<char>,
    carry: Option<ClusterCarry>,
    carry_text: String,
    scratch: String,
    dcs_query: Option<DcsQuery>,
    dcs_payload: Vec<u8>,
    selection: Option<Selection>,
    lines_produced: u64,
    /// Cell size in pixels, `(width, height)`, `(0, 0)` until the embedder sets it; images and
    /// `CSI 14 t` read the effective cell.
    cell_pixels: (u16, u16),
    graphics: GraphicsState,
    /// Scratch for the style and extras sweeps, kept for its capacity.
    live_ids: FxHashSet<u32>,
    stats: FeedStats,
    dispatched: bool,
}

/// Which designated set the next printed character uses.
#[derive(Clone, Copy, Debug, Default)]
struct Shifts {
    /// The GL set (SI, SO, LS2, LS3).
    active: u8,
    /// A pending SS2 or SS3, which applies to one character.
    single: Option<u8>,
    /// What DECSC saved, per screen: index 0 primary, 1 alternate.
    saved: [(u8, Option<u8>); 2],
}

/// The counters `feed` reads before parsing and differences afterwards.
#[derive(Clone, Copy)]
struct Baselines {
    hyperlinks: u32,
    styles: u32,
    truncated: u32,
    exhausted: u32,
    wide: u32,
}

impl State {
    fn baselines(&self) -> Baselines {
        let interner = &self.interner;
        Baselines {
            hyperlinks: interner
                .hyperlinks
                .exhausted()
                .saturating_add(interner.extras.exhausted()),
            styles: interner.styles.exhausted(),
            truncated: interner.graphemes.truncated(),
            exhausted: interner.graphemes.exhausted(),
            wide: self
                .grid
                .primary()
                .dropped_wide()
                .saturating_add(self.grid.alt().dropped_wide()),
        }
    }

    /// Adds the counters that grew since `before` to this feed's stats.
    fn add_deltas(&mut self, before: Baselines) {
        let now = self.baselines();
        let stats = &mut self.stats;
        let add = |counter: &mut u32, now: u32, before: u32| {
            *counter = counter.saturating_add(now.saturating_sub(before));
        };
        add(
            &mut stats.hyperlink_table_exhausted,
            now.hyperlinks,
            before.hyperlinks,
        );
        add(&mut stats.style_table_exhausted, now.styles, before.styles);
        add(
            &mut stats.grapheme_truncated,
            now.truncated,
            before.truncated,
        );
        add(
            &mut stats.grapheme_table_exhausted,
            now.exhausted,
            before.exhausted,
        );
        add(&mut stats.wide_glyphs_dropped, now.wide, before.wide);
    }

    /// The modes the print path reads.
    fn print_mode(&self) -> PrintMode {
        PrintMode {
            insert: self.modes.get(Mode::Insert),
            autowrap: self.modes.get(Mode::LineWrap),
            reverse_wrap: self.modes.get(Mode::ReverseWrap),
        }
    }

    /// Releases the selection when asking `inv` says an erase about to run would blank it.
    /// Ask before the erase.
    fn invalidate(&mut self, inv: Invalidation) {
        if self
            .selection
            .as_ref()
            .is_some_and(|selection| selection.invalidated_by(&self.grid, inv))
        {
            self.release_selection();
        }
    }

    fn release_selection(&mut self) {
        if let Some(selection) = self.selection.take() {
            selection.release(&mut self.grid);
        }
    }

    /// Releases the selection when one end's content is gone. A fresh empty selection stays.
    fn prune_selection(&mut self) {
        if self
            .selection
            .as_ref()
            .is_some_and(|selection| selection.is_dead(&self.grid))
        {
            self.release_selection();
        }
    }

    /// Marks a change to rows that the per-row stamps do not cover; every snapshot state
    /// rebuilds on its next update.
    fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// Marks a change to an effective colour when `changed`; every snapshot state rebuilds and
    /// remaps on its next update.
    fn note_palette_change(&mut self, changed: bool) {
        if changed {
            self.palette_epoch = self.palette_epoch.wrapping_add(1);
        }
    }

    /// Index into `shifts.saved` for the active screen.
    fn saved_slot(&self) -> usize {
        usize::from(self.grid.alt_active())
    }

    /// DECSC: the cursor with its pens and designations, plus the shift state.
    fn save_cursor(&mut self) {
        self.grid.screen_mut().save_cursor();
        let slot = self.saved_slot();
        let now = (self.shifts.active, self.shifts.single);
        if let Some(saved) = self.shifts.saved.get_mut(slot) {
            *saved = now;
        }
    }

    /// DECRC: the counterpart of [`save_cursor`](Self::save_cursor).
    fn restore_cursor(&mut self) {
        self.grid.screen_mut().restore_cursor();
        if let Some(&(active, single)) = self.shifts.saved.get(self.saved_slot()) {
            self.shifts.active = active;
            self.shifts.single = single;
        }
    }

    fn charset(&self, set: u8) -> Charset {
        self.grid
            .screen()
            .cursor()
            .charsets
            .get(usize::from(set))
            .copied()
            .unwrap_or_default()
    }
}

impl Terminal {
    /// A terminal at power-on state: `size` is clamped to `1..=1024` rows and `1..=2048`
    /// columns; `config.product_name` is cleaned (see [`Config::product_name`]).
    pub fn new(size: Size, mut config: Config) -> Terminal {
        config.product_name = query::sanitize_product_name(config.product_name.take());
        let state = State {
            grid: TerminalGrid::new(size, config.scrollback_limit),
            interner: Interner::default(),
            config,
            modes: Modes::default(),
            cursor_style: None,
            keyboard: KeyboardStacks::default(),
            modify_other_keys: 0,
            title: TitleStack::default(),
            colors: ColorOverrides::default(),
            theme: Palette::new(),
            sync: SyncState::new(),
            generation: 0,
            palette_epoch: 0,
            shifts: Shifts::default(),
            preceding_char: None,
            carry: None,
            carry_text: String::new(),
            scratch: String::new(),
            dcs_query: None,
            dcs_payload: Vec::new(),
            selection: None,
            lines_produced: 0,
            cell_pixels: (0, 0),
            graphics: GraphicsState::new(),
            live_ids: FxHashSet::default(),
            stats: FeedStats::default(),
            dispatched: false,
        };
        Terminal {
            parser: Parser::new(),
            state,
        }
    }

    /// Parses `bytes` and applies them: clears `batch`, fills it with the events in byte order
    /// and returns this call's counters (also kept for [`stats`](Self::stats)).
    ///
    /// Any chunking of one stream gives the same screen and the same events, except where a
    /// `GraphicReleased` lands: images are swept at the end of each call. `now` times
    /// synchronized output (`?2026`): a block opened here holds frames from `now` on (see
    /// [`SyncState`]). The engine reads no clock itself.
    #[cfg_attr(
        feature = "hotpath-profiling",
        hotpath::measure(impl_type = "Terminal")
    )]
    pub fn feed(&mut self, bytes: &[u8], batch: &mut EventBatch, now: Instant) -> FeedStats {
        batch.clear();
        let state = &mut self.state;
        state.stats = FeedStats {
            bytes: bytes.len(),
            ..FeedStats::default()
        };
        state.dispatched = false;
        if bytes.is_empty() {
            // A resize may have released images since the last feed; deliver them here.
            state.graphics.drain_released(batch);
            return state.stats;
        }
        let before = state.baselines();
        state.grid.begin_batch();
        let mut handler = Handler {
            state: &mut *state,
            out: &mut *batch,
            now,
        };
        self.parser.advance(&mut handler, bytes);
        state.grid.sync_anchors();
        state.prune_selection();
        if state.interner.needs_grapheme_sweep() {
            state.grid.sweep_graphemes(&mut state.interner.graphemes);
        }
        if !state.graphics.placements.is_empty() {
            state.graphics.sweep(&mut state.grid);
        }
        state.graphics.drain_released(batch);
        state.sweep_extras_if_due();
        state.add_deltas(before);
        state.stats.unhandled_sequences = state
            .stats
            .unhandled_sequences
            .saturating_add(batch.overflowed());
        if state.dispatched {
            batch.push_repaint();
        }
        state.grid.debug_check_with(&state.interner);
        state.stats
    }

    /// Resizes both screens (see [`TerminalGrid::resize`]), drops a pending `?2027` cluster and
    /// releases a selection whose content was lost.
    pub fn resize(&mut self, size: Size, policy: ResizePolicy) -> ResizeOutcome {
        let state = &mut self.state;
        let outcome = state.grid.resize(size, policy);
        state.bump_generation();
        state.carry = None;
        state.prune_selection();
        state.graphics.sweep(&mut state.grid);
        outcome
    }

    /// Changes the primary screen's history limit (clamped to 1,000,000) and stores it in the
    /// config. A lower limit trims the oldest rows at once, which kills selections on them.
    pub fn set_scrollback_limit(&mut self, limit: u32) {
        let state = &mut self.state;
        state.config.scrollback_limit = limit;
        state.grid.set_scrollback_limit(limit);
        state.bump_generation();
        state.prune_selection();
    }

    /// Scrolls the viewport of the active screen by `delta` rows: negative towards history,
    /// positive towards output, clamped. `scroll_viewport(i32::MAX)` shows the bottom again.
    /// Equivalent to `grid_mut().screen_mut().scroll_viewport(delta)` followed by
    /// `grid_mut().sync_anchors()`.
    pub fn scroll_viewport(&mut self, delta: i32) {
        self.state.grid.screen_mut().scroll_viewport(delta);
        self.state.grid.sync_anchors();
    }

    /// The grid, for reading rows.
    pub fn grid(&self) -> &TerminalGrid {
        &self.state.grid
    }

    /// Both screens, mutably: write rows, move the viewport, open a batch.
    ///
    /// - Writes here bypass the parser: the selection, image placements, [`ColorOverrides`], the
    ///   title and the generation counter are not told. Fixtures, replay tools and viewport
    ///   control are the uses.
    /// - `Screen::row_mut` stamps the row with the current batch. A [`SnapshotState`] that already
    ///   saw that batch skips it: call `grid_mut().begin_batch()` before writing so the next
    ///   update copies the row.
    /// - Moving the viewport by hand is `screen_mut().scroll_viewport(d)` (or
    ///   `scroll_to_bottom()`) followed by `sync_anchors()`, so a resize before the next `feed`
    ///   keeps the new viewport top. [`scroll_viewport`](Self::scroll_viewport) does both.
    /// - Resize and reset go through [`resize`](Self::resize) and a `RIS` feed, not through
    ///   `TerminalGrid::resize` / `reset` here: those skip selection pruning, the placement
    ///   sweep, the pending-cluster drop and the generation bump.
    /// - A `GraphicId` in a hand-built `Extras` must come from the engine (see
    ///   [`VtEvent::GraphicReleased`](crate::VtEvent::GraphicReleased) and the snapshot
    ///   placements); an id made up by the caller never resolves.
    /// - Nothing panics: `row_mut` returns `None` outside the live range, counts clamp.
    pub fn grid_mut(&mut self) -> &mut TerminalGrid {
        &mut self.state.grid
    }

    /// The active screen.
    pub fn screen(&self) -> &Screen {
        self.state.grid.screen()
    }

    /// The tables cells point into.
    pub fn interner(&self) -> &Interner {
        &self.state.interner
    }

    /// The intern tables, mutably: mint the ids a hand-written cell carries.
    ///
    /// - Ids minted here stay valid while a cell in the grid carries them: `feed` sweeps
    ///   graphemes, styles and extras no row uses (a live grapheme id may be renumbered, its cells
    ///   rewritten, or dropped to its first scalar past half the arena). Mint and write in one
    ///   step, before the next `feed`.
    /// - A full table answers with its fallback id (`StyleId::DEFAULT`, `ExtrasId::NONE`,
    ///   grapheme id 0; `hyperlinks.intern` answers `None`) and counts it in the table's own
    ///   `exhausted()` / `truncated()`, not in [`FeedStats`].
    pub fn interner_mut(&mut self) -> &mut Interner {
        &mut self.state.interner
    }

    /// The config, with the product name as cleaned by [`new`](Self::new).
    pub fn config(&self) -> &Config {
        &self.state.config
    }

    /// The counters of the last [`feed`](Self::feed).
    pub fn stats(&self) -> FeedStats {
        self.state.stats
    }

    /// The size of the active screen.
    pub fn size(&self) -> Size {
        self.state.grid.screen().size()
    }

    /// The rows the user sees on the active screen.
    pub fn viewport(&self) -> Viewport {
        self.state.grid.screen().viewport()
    }

    /// Whether `mode` is on.
    ///
    /// The alternate-screen modes report whether the alternate screen is shown, `CursorBlink`
    /// the blink of the current cursor style, and the inert `DecCoLm` and `SaveCursor1048`
    /// are always `false`.
    pub fn mode(&self, mode: Mode) -> bool {
        self.state.mode(mode)
    }

    /// The cursor style: what DECSCUSR, OSC 50 and `?12` set, else the configured default.
    /// With the cursor hidden (`?25` reset) the shape is [`CursorShape::Hidden`] and the
    /// blink is kept.
    pub fn cursor_style(&self) -> CursorStyle {
        self.state.cursor_style()
    }

    /// The kitty keyboard flags in force on the active screen.
    pub fn keyboard_flags(&self) -> KeyboardFlags {
        self.state.keyboard.active.live()
    }

    /// The `modifyOtherKeys` level, `0..=2`.
    pub fn modify_other_keys(&self) -> u8 {
        self.state.modify_other_keys
    }

    /// The window title set by OSC 0 or 2, if any.
    pub fn title(&self) -> Option<&str> {
        self.state.title.title()
    }

    /// How many titles `CSI 22 t` has pushed (at most 16).
    pub fn title_depth(&self) -> usize {
        self.state.title.depth()
    }

    /// The colour a program set with OSC 4, 10, 11, 12, 17 or 19, if any. `None` means the
    /// embedder's theme owns the colour.
    pub fn color(&self, key: ColorKey) -> Option<Rgb> {
        self.state.colors.get(key)
    }

    /// The bytes that answer a [`VtEvent::ColorQuery`](crate::VtEvent::ColorQuery) for `key`
    /// from this terminal's own colours: the program's override when it set one, else the
    /// theme of [`set_theme_colors`](Self::set_theme_colors). Echo the query's `terminator`.
    ///
    /// `None` for a selection colour no program set: the theme has no slot for it, so answer
    /// with [`ColorKey::color_reply`] from your own colours, or stay silent.
    pub fn color_reply(&self, key: ColorKey, terminator: StringTerm) -> Option<Vec<u8>> {
        let theme = &self.state.theme;
        let themed = match key {
            ColorKey::Palette(index) => theme.indexed.get(usize::from(index)).copied(),
            ColorKey::Foreground => Some(theme.foreground),
            ColorKey::Background => Some(theme.background),
            ColorKey::Cursor => Some(theme.cursor),
            ColorKey::SelectionBackground | ColorKey::SelectionForeground => None,
        };
        let color = self.state.colors.get(key).or(themed)?;
        Some(key.color_reply(color, terminator))
    }

    /// Every colour override.
    pub fn colors(&self) -> &ColorOverrides {
        &self.state.colors
    }

    /// Sets the embedder's colours: what the default colours and the 256 indexed colours are
    /// until a program overrides one with OSC 4, 10, 11 or 12 (a reset returns the slot here).
    ///
    /// A theme that differs from the current one makes the next update of every
    /// [`SnapshotState`] a `Full` and remaps every row; the same theme again does nothing, so
    /// calling this every frame is cheap. Apply the theme with
    /// [`SnapshotState::map_colors`] and [`palette`](Self::palette).
    pub fn set_theme_colors(&mut self, theme: &Palette) {
        let state = &mut self.state;
        let changed = state.theme != *theme;
        state.theme = *theme;
        state.note_palette_change(changed);
    }

    /// The effective palette: the theme with every program override applied. Pass it to
    /// [`SnapshotState::map_colors`]. Selection colours have no slot here; read them with
    /// [`color`](Self::color).
    pub fn palette(&self) -> Palette {
        let state = &self.state;
        let mut palette = state.theme;
        for (index, slot) in palette.indexed.iter_mut().enumerate() {
            if let Some(color) = u8::try_from(index)
                .ok()
                .and_then(|index| state.colors.get(ColorKey::Palette(index)))
            {
                *slot = color;
            }
        }
        let over = |key: ColorKey, slot: &mut Rgb| {
            if let Some(color) = state.colors.get(key) {
                *slot = color;
            }
        };
        over(ColorKey::Foreground, &mut palette.foreground);
        over(ColorKey::Background, &mut palette.background);
        over(ColorKey::Cursor, &mut palette.cursor);
        palette
    }

    /// The synchronized-output state (`?2026`). Read-only: the watchdog fires only inside
    /// [`snapshot_update`](Self::snapshot_update).
    pub fn sync(&self) -> &SyncState {
        &self.state.sync
    }

    /// The modes a renderer or an input encoder reads, as of now.
    pub fn mode_snapshot(&self) -> ModeSnapshot {
        self.state.mode_snapshot()
    }

    /// The bytes to send for a key press against this terminal's own modes (`None`: send
    /// nothing). They change as the program negotiates cursor-key mode, kitty keyboard flags
    /// or `modifyOtherKeys`; see [`input::encode_key`].
    pub fn encode_key(&self, key: &KeySpec, mods: KeyMods) -> Option<Vec<u8>> {
        input::encode_key(key, mods, &self.mode_snapshot())
    }

    /// The bytes to send for a key event against this terminal's own modes (`None`: send
    /// nothing). They change as the program negotiates cursor-key mode, kitty keyboard flags
    /// or `modifyOtherKeys`; see [`input::encode_key_event`].
    pub fn encode_key_event(&self, event: &KeyEvent) -> Option<Vec<u8>> {
        input::encode_key_event(event, &self.mode_snapshot())
    }

    /// The bytes to send for pasted `text` against this terminal's own modes: bracketed when
    /// the program asked for it (`?2004`), control characters filtered either way. Empty
    /// when nothing is left to send; see [`input::encode_paste`].
    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        input::encode_paste(text, &self.mode_snapshot())
    }

    /// The bytes to send when the window gains (`focused`) or loses the keyboard focus;
    /// `None` unless the program asked for focus reports (`?1004`). See
    /// [`input::encode_focus`].
    pub fn encode_focus(&self, focused: bool) -> Option<&'static [u8]> {
        input::encode_focus(focused, &self.mode_snapshot())
    }

    /// The mouse protocol a program asked for, if any; the same as
    /// [`mode_snapshot`](Self::mode_snapshot)`().mouse`.
    pub fn mouse_reporting(&self) -> Option<MouseProtocol> {
        self.state.modes.mouse_protocol()
    }

    /// Brings `state` up to date and says what changed (phase 1 of drawing; hold your lock).
    ///
    /// Only rows written since the state's watermark are copied. While synchronized output
    /// holds frames, a state that was built before gets `Unchanged` with its cursor, selection
    /// and modes still refreshed, and a change they went through is reported once by the next
    /// real frame. `now` is the time deadlines are checked against; `&mut self` because the
    /// watchdog may close the block here. Call [`SnapshotState::map_colors`] afterwards.
    #[cfg_attr(
        feature = "hotpath-profiling",
        hotpath::measure(impl_type = "Terminal")
    )]
    pub fn snapshot_update(&mut self, state: &mut SnapshotState, now: Instant) -> SnapshotUpdate {
        let modes = self.mode_snapshot();
        let selection = self.selection_range();
        let engine = &mut self.state;
        let view = EngineView {
            grid: &engine.grid,
            interner: &engine.interner,
            sync: &mut engine.sync,
            modes,
            generation: engine.generation,
            palette_epoch: engine.palette_epoch,
            selection,
            placements: &engine.graphics.placements,
        };
        state.begin_update(view, now)
    }

    /// Takes the images decoded since the last call, oldest first. This is the only drain:
    /// several [`SnapshotState`]s may exist, and a frame skipped by `?2026` loses nothing.
    ///
    /// Each [`GraphicData`] is handed out once. The order for one cycle is
    /// [`feed`](Self::feed), `take_graphics` (upload the pixels, keyed by
    /// [`GraphicData::id`]), handle the batch (a
    /// [`GraphicReleased`](crate::VtEvent::GraphicReleased) event frees that texture), then
    /// [`snapshot_update`](Self::snapshot_update). An image released before it was taken is still
    /// handed out here once, so the release that follows frees it. At most 128 MiB of pixels
    /// wait; past that the oldest are dropped and counted in [`FeedStats::graphics_dropped`].
    pub fn take_graphics(&mut self) -> Vec<Arc<GraphicData>> {
        let graphics = &mut self.state.graphics;
        graphics.pending_bytes = 0;
        std::mem::take(&mut graphics.pending)
    }

    /// The live image placements, oldest first (draw order). Read-only; a renderer usually
    /// reads them through [`SnapshotState::placements`] instead, which resolves each anchor
    /// to a row and column.
    pub fn placements(&self) -> &[Placement] {
        &self.state.graphics.placements
    }

    /// Tells the engine how big a cell is in device pixels. It sizes the footprint of images
    /// placed from now on (`ceil(pixels / cell)` per axis) and the answer to `CSI 14 t`. A
    /// zero axis means unknown: [`VIRTUAL_CELL`](crate::VIRTUAL_CELL) is used for both.
    /// Images already placed keep their footprint; call this on every font or DPI change.
    pub fn set_cell_pixels(&mut self, width: u16, height: u16) {
        self.state.cell_pixels = (width, height);
    }

    /// The pen: the cell a printed glyph inherits (style, hyperlink, semantic zone).
    pub fn template(&self) -> Cell {
        self.state.grid.screen().cursor().template()
    }

    /// The pen's style.
    pub fn style(&self) -> Style {
        *self
            .state
            .interner
            .resolve_style(self.template().style_id())
    }

    /// The character set in the GL slot now.
    pub fn active_charset(&self) -> Charset {
        self.state.charset(self.state.shifts.active)
    }

    /// Output lines produced since power-on: one per line feed, vertical tab, form feed, IND
    /// and NEL (scrolling or not), plus the rows SU and DL pushed into history. Wraps, ED 2
    /// and screen switches add nothing; a reset keeps the count.
    pub fn lines_produced(&self) -> u64 {
        self.state.lines_produced
    }

    /// The text of row `id` on its screen: no trimming, no newline, spacers skipped.
    pub fn row_text(&self, id: RowId) -> String {
        let mut text = String::new();
        self.state
            .grid
            .screen_of(id)
            .row_text(id, &self.state.interner.graphemes, &mut text);
        text
    }

    /// The nearest row above `row` on the active screen where a shell prompt starts, for
    /// "jump to the previous prompt"; pass [`viewport`](Self::viewport)`().top`. `None`
    /// when there is none.
    ///
    /// A prompt is found by its cells: the ones printed between OSC 133 `A` and `B` carry
    /// [`Semantic::Prompt`](crate::Semantic). A prompt of several rows counts once, at its
    /// first row; a prompt that printed nothing is not found. The search walks the history
    /// (about a millisecond for 10,000 rows): call it on a key press, not per frame.
    pub fn prompt_before(&self, row: RowId) -> Option<RowId> {
        let screen = self.state.grid.screen();
        let mut id = row.min(screen.newest() + 1);
        while id > screen.oldest() {
            id = id - 1;
            if starts_prompt(screen, id) {
                return Some(id);
            }
        }
        None
    }

    /// The nearest row below `row` on the active screen where a shell prompt starts, for
    /// "jump to the next prompt". See [`prompt_before`](Self::prompt_before).
    pub fn prompt_after(&self, row: RowId) -> Option<RowId> {
        let screen = self.state.grid.screen();
        let mut id = row.max(screen.oldest()) + 1;
        while id <= screen.newest() {
            if starts_prompt(screen, id) {
                return Some(id);
            }
            id = id + 1;
        }
        None
    }

    /// Turns a pointer position into a cell and the half of it that was hit. Rows are counted
    /// from the top of the viewport and columns from its left edge, both fractional; negative
    /// and NaN values count as 0 and positions past the edges clamp. A column past the last
    /// one is `Side::Right`.
    pub fn hit_test(&self, viewport_row: f32, col: f32) -> (Pos, Side) {
        let viewport = self.viewport();
        let clean = |value: f32| if value.is_nan() { 0.0 } else { value.max(0.0) };
        // Float to integer casts saturate, then the clamp bounds them to the screen.
        let row = (clean(viewport_row) as u16).min(viewport.rows.saturating_sub(1));
        let col = clean(col);
        let last = viewport.cols.saturating_sub(1);
        let (col_index, side) = if col >= f32::from(viewport.cols) {
            (last, Side::Right)
        } else if col.fract() < 0.5 {
            (col as u16, Side::Left)
        } else {
            (col as u16, Side::Right)
        };
        let pos = Pos {
            row: viewport.top + u64::from(row),
            col: col_index.min(last),
        };
        (pos, side)
    }

    /// Starts a selection of `kind` at `pos`, releasing any earlier one.
    pub fn selection_start(&mut self, pos: Pos, side: Side, kind: SelectionKind) {
        let state = &mut self.state;
        state.release_selection();
        state.selection = Some(Selection::new(&mut state.grid, kind, pos, side));
    }

    /// Moves the end of the selection; does nothing without one.
    pub fn selection_update(&mut self, pos: Pos, side: Side) {
        let state = &mut self.state;
        if let Some(selection) = &mut state.selection {
            selection.update(&mut state.grid, pos, side);
        }
    }

    /// The cells selected now, if any.
    pub fn selection_range(&self) -> Option<SelectionRange> {
        let state = &self.state;
        state
            .selection
            .as_ref()?
            .to_range(&state.grid, &state.config.semantic_escape_chars)
    }

    /// Whether the selection covers at least one cell.
    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    /// The selected text, if any.
    pub fn selection_text(&self) -> Option<String> {
        let state = &self.state;
        state.selection.as_ref()?.text(
            &state.grid,
            &state.interner.graphemes,
            &state.config.semantic_escape_chars,
        )
    }

    /// Drops the selection; harmless without one.
    pub fn selection_clear(&mut self) {
        self.state.release_selection();
    }

    /// Selects everything on the active screen, history included, releasing any earlier
    /// selection.
    pub fn select_all(&mut self) {
        let state = &mut self.state;
        state.release_selection();
        state.selection = Some(Selection::all(&mut state.grid));
    }
}

/// Whether a shell prompt starts on row `id`: it holds a prompt cell that does not continue
/// the prompt of the row above.
fn starts_prompt(screen: &Screen, id: RowId) -> bool {
    let is_prompt = |cell: &Cell| cell.semantic() == Semantic::Prompt;
    let row = screen.row(id);
    // Cells from `occ` on equal the last one, so the written prefix and the last cell hold
    // every zone of the row.
    let cells = row.cells();
    let written = cells.get(..usize::from(row.occ())).unwrap_or(cells);
    let first = written.iter().position(is_prompt).or_else(|| {
        cells
            .last()
            .filter(|cell| is_prompt(cell))
            .map(|_| written.len())
    });
    match first {
        None => false,
        // The cell before it on this row is no prompt: the prompt starts here.
        Some(col) if col > 0 => true,
        Some(_) if id <= screen.oldest() => true,
        Some(_) => {
            let above = screen.row(id - 1);
            let end = usize::from(above.text_end());
            let continues = end > 0 && above.cells().get(end - 1).is_some_and(is_prompt);
            !continues
        }
    }
}

impl fmt::Debug for Terminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Terminal")
            .field("size", &self.size())
            .field("alt_active", &self.state.grid.alt_active())
            .field("title", &self.title())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
impl Terminal {
    /// Replaces the style table of a fresh terminal with one of `capacity` entries, so a
    /// test fills it with thousands of styles instead of a million.
    pub(crate) fn limit_styles(&mut self, capacity: usize) {
        assert_eq!(self.state.interner.styles.entries(), 1, "a fresh terminal");
        self.state.interner.styles = crate::intern::InternTable::with_capacity(capacity);
    }

    /// The change stamp of a primary-screen row.
    fn primary_row_seq(&self, id: RowId) -> crate::grid::SeqNo {
        self.state.grid.primary().row(id).seq()
    }
}
