//! The terminal: bytes in, grid changes, events and counters out.
//!
//! [`Terminal::feed`] runs the parser over output, applies every sequence to the grid and the
//! mode state, and appends [`VtEvent`](crate::VtEvent)s to a caller-owned
//! [`EventBatch`](crate::EventBatch). Nothing here runs embedder code, performs I/O or takes a
//! lock; answers to queries are events the embedder writes back. Input is untrusted: what the
//! engine cannot honour is dropped and counted in [`FeedStats`](crate::FeedStats).

use std::borrow::Cow;
use std::fmt;
use std::time::Instant;

use crate::cell::{Cell, Rgb, Style};
use crate::events::{EventBatch, FeedStats};
use crate::grid::terminal_grid::{ResizeOutcome, ResizePolicy};
use crate::grid::{
    Charset, DEFAULT_SCROLLBACK, Pos, PrintMode, RowId, Screen, Size, TerminalGrid, Viewport,
};
use crate::intern::Interner;
use crate::parser::Parser;
use crate::selection::{
    DEFAULT_SEMANTIC_ESCAPE_CHARS, Invalidation, Selection, SelectionKind, SelectionRange, Side,
};

mod color;
mod csi;
mod dispatch;
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
/// bump, and that form keeps compiling.
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
    /// Cell size in pixels, `(width, height)`; read by `CSI 14 t`.
    cell_pixels: (u16, u16),
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
    /// Any chunking of one stream gives the same screen and the same events. `now` is unused
    /// until synchronized output lands.
    pub fn feed(&mut self, bytes: &[u8], batch: &mut EventBatch, _now: Instant) -> FeedStats {
        batch.clear();
        let state = &mut self.state;
        state.stats = FeedStats {
            bytes: bytes.len(),
            ..FeedStats::default()
        };
        state.dispatched = false;
        if bytes.is_empty() {
            return state.stats;
        }
        let before = state.baselines();
        state.grid.begin_batch();
        let mut handler = Handler {
            state: &mut *state,
            out: &mut *batch,
        };
        self.parser.advance(&mut handler, bytes);
        state.grid.sync_anchors();
        state.prune_selection();
        if state.interner.needs_grapheme_sweep() {
            state.grid.sweep_graphemes(&mut state.interner.graphemes);
        }
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
        state.carry = None;
        state.prune_selection();
        outcome
    }

    /// Changes the primary screen's history limit (clamped to 1,000,000) and stores it in the
    /// config. A lower limit trims the oldest rows at once, which kills selections on them.
    pub fn set_scrollback_limit(&mut self, limit: u32) {
        let state = &mut self.state;
        state.config.scrollback_limit = limit;
        state.grid.set_scrollback_limit(limit);
        state.prune_selection();
    }

    /// Scrolls the viewport of the active screen by `delta` rows: negative towards history,
    /// positive towards output, clamped. `scroll_viewport(i32::MAX)` shows the bottom again.
    pub fn scroll_viewport(&mut self, delta: i32) {
        self.state.grid.screen_mut().scroll_viewport(delta);
        self.state.grid.sync_anchors();
    }

    /// The grid, for reading rows.
    pub fn grid(&self) -> &TerminalGrid {
        &self.state.grid
    }

    /// The active screen.
    pub fn screen(&self) -> &Screen {
        self.state.grid.screen()
    }

    /// The tables cells point into.
    pub fn interner(&self) -> &Interner {
        &self.state.interner
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

    /// Every colour override.
    pub fn colors(&self) -> &ColorOverrides {
        &self.state.colors
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
    /// The change stamp of a primary-screen row.
    fn primary_row_seq(&self, id: RowId) -> crate::grid::SeqNo {
        self.state.grid.primary().row(id).seq()
    }
}
