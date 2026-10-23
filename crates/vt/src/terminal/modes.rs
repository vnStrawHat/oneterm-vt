//! Modes, the cursor style, keyboard protocol state, the title stack, the alternate screens
//! and the two resets.

use bitflags::bitflags;

use crate::cell::Cell;
use crate::events::VtEvent;
use crate::grid::CursorOrigin;
use crate::selection::Invalidation;

use super::csi::Args;
use super::dispatch::Handler;
use super::{Shifts, State};

/// Titles `CSI 22 t` keeps; pushing past it drops the oldest.
pub(super) const TITLE_STACK_MAX: usize = 16;
/// Kitty keyboard flag entries kept per screen; pushing past it drops the oldest.
pub(super) const KEYBOARD_STACK_MAX: usize = 8;

/// A terminal mode a program can set or reset, or ask about with DECRQM.
///
/// The doc of each variant names the sequence. Modes the engine only stores for the embedder
/// (mouse, focus, bracketed paste) change nothing in the grid; the embedder reads them.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mode {
    /// `?1`: application cursor keys.
    AppCursor,
    /// `?3`: 80/132 columns. Inert: setting or resetting it clears the screen and homes the
    /// cursor, but the width never changes.
    DecCoLm,
    /// `?5`: reverse video, a flag for the renderer; no cell changes.
    ReverseVideo,
    /// `?6`: origin mode; the cursor is relative to the scroll region and homes on every change.
    Origin,
    /// `?7`: auto-wrap. On at power-on.
    LineWrap,
    /// `?9`: X10 mouse reporting.
    MouseX10,
    /// `?1000`: normal mouse tracking.
    MouseClick,
    /// `?1002`: button-event mouse tracking.
    MouseDrag,
    /// `?1003`: any-event mouse tracking.
    MouseMotion,
    /// `?12`: cursor blink; lives in the cursor style.
    CursorBlink,
    /// `?25`: show the cursor. On at power-on.
    ShowCursor,
    /// `?45`: reverse wrap for backspace.
    ReverseWrap,
    /// `?47`: alternate screen, no cursor save.
    AltScreen47,
    /// `?1047`: alternate screen, cleared on leave.
    AltScreen1047,
    /// `?1049`: alternate screen with cursor save and clear.
    AltScreen,
    /// `?1004`: focus in/out reports.
    FocusInOut,
    /// `?1005`: UTF-8 mouse encoding.
    Utf8Mouse,
    /// `?1006`: SGR mouse encoding.
    SgrMouse,
    /// `?1015`: urxvt mouse encoding.
    UrxvtMouse,
    /// `?1007`: alternate scroll (wheel as arrow keys on the alternate screen). On at power-on.
    AlternateScroll,
    /// `?1042`: urgency hint on bell. On at power-on.
    UrgencyHints,
    /// `?1048`: save (set) or restore (reset) the cursor. Inert: it keeps no state.
    SaveCursor1048,
    /// `?2004`: bracketed paste.
    BracketedPaste,
    /// `?2026`: synchronized output; stored only.
    SyncUpdate,
    /// `?2027`: grapheme clustering; a cluster takes one cell.
    GraphemeClusters,
    /// `?9001`: Win32 input mode. Accepted and stored, answers "reset".
    Win32Input,
    /// `4`: insert mode (IRM).
    Insert,
    /// `20`: line feed / new line. Accepted and stored, answers "reset"; a line feed never
    /// implies a carriage return.
    LineFeedNewLine,
    /// `ESC =` and `ESC >`: application keypad; has no DECRQM number.
    AppKeypad,
}

/// The five answers DECRQM can give.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum ModeState {
    /// The mode is not recognised.
    NotSupported = 0,
    /// The mode is set.
    Set = 1,
    /// The mode is reset.
    Reset = 2,
    /// The mode is set and cannot be changed.
    PermanentlySet = 3,
    /// The mode is reset and cannot be changed.
    PermanentlyReset = 4,
}

/// Private modes (`CSI ? Pm h/l`) and their numbers.
const PRIVATE_MODES: &[(u16, Mode)] = &[
    (1, Mode::AppCursor),
    (3, Mode::DecCoLm),
    (5, Mode::ReverseVideo),
    (6, Mode::Origin),
    (7, Mode::LineWrap),
    (9, Mode::MouseX10),
    (12, Mode::CursorBlink),
    (25, Mode::ShowCursor),
    (45, Mode::ReverseWrap),
    (47, Mode::AltScreen47),
    (1000, Mode::MouseClick),
    (1002, Mode::MouseDrag),
    (1003, Mode::MouseMotion),
    (1004, Mode::FocusInOut),
    (1005, Mode::Utf8Mouse),
    (1006, Mode::SgrMouse),
    (1007, Mode::AlternateScroll),
    (1015, Mode::UrxvtMouse),
    (1042, Mode::UrgencyHints),
    (1047, Mode::AltScreen1047),
    (1048, Mode::SaveCursor1048),
    (1049, Mode::AltScreen),
    (2004, Mode::BracketedPaste),
    (2026, Mode::SyncUpdate),
    (2027, Mode::GraphemeClusters),
    (9001, Mode::Win32Input),
];

/// ANSI modes (`CSI Pm h/l`) and their numbers.
const ANSI_MODES: &[(u16, Mode)] = &[(4, Mode::Insert), (20, Mode::LineFeedNewLine)];

/// The mouse tracking modes; at most one is on.
const MOUSE_PROTOCOLS: [Mode; 4] = [
    Mode::MouseX10,
    Mode::MouseClick,
    Mode::MouseDrag,
    Mode::MouseMotion,
];
/// The mouse encoding modes; at most one is on.
const MOUSE_ENCODINGS: [Mode; 3] = [Mode::Utf8Mouse, Mode::SgrMouse, Mode::UrxvtMouse];

impl Mode {
    /// The mode behind a private mode number.
    pub(super) fn from_private(number: u16) -> Option<Mode> {
        lookup(PRIVATE_MODES, number)
    }

    /// The mode behind an ANSI mode number.
    pub(super) fn from_ansi(number: u16) -> Option<Mode> {
        lookup(ANSI_MODES, number)
    }

    /// The bit a stateful mode owns in [`Modes`]; 0 for a mode that keeps no bit.
    const fn bit(self) -> u32 {
        match self {
            Mode::AppCursor => 1 << 0,
            Mode::ReverseVideo => 1 << 1,
            Mode::Origin => 1 << 2,
            Mode::LineWrap => 1 << 3,
            Mode::MouseX10 => 1 << 4,
            Mode::MouseClick => 1 << 5,
            Mode::MouseDrag => 1 << 6,
            Mode::MouseMotion => 1 << 7,
            Mode::ShowCursor => 1 << 8,
            Mode::ReverseWrap => 1 << 9,
            Mode::FocusInOut => 1 << 10,
            Mode::Utf8Mouse => 1 << 11,
            Mode::SgrMouse => 1 << 12,
            Mode::UrxvtMouse => 1 << 13,
            Mode::AlternateScroll => 1 << 14,
            Mode::UrgencyHints => 1 << 15,
            Mode::BracketedPaste => 1 << 16,
            Mode::SyncUpdate => 1 << 17,
            Mode::GraphemeClusters => 1 << 18,
            Mode::Win32Input => 1 << 19,
            Mode::Insert => 1 << 20,
            Mode::LineFeedNewLine => 1 << 21,
            Mode::AppKeypad => 1 << 22,
            Mode::DecCoLm
            | Mode::CursorBlink
            | Mode::AltScreen47
            | Mode::AltScreen1047
            | Mode::AltScreen
            | Mode::SaveCursor1048 => 0,
        }
    }

    /// The fixed answer DECRQM gives for a mode nothing reads, or `None` for a live mode.
    ///
    /// DECRQM never answers "set" for a mode the engine does not act on.
    pub const fn inert_state(self) -> Option<ModeState> {
        match self {
            Mode::DecCoLm => Some(ModeState::NotSupported),
            Mode::Win32Input | Mode::LineFeedNewLine => Some(ModeState::Reset),
            _ => None,
        }
    }
}

fn lookup(table: &[(u16, Mode)], number: u16) -> Option<Mode> {
    table
        .iter()
        .find(|(candidate, _)| *candidate == number)
        .map(|(_, mode)| *mode)
}

/// One bit per stateful mode.
#[derive(Clone, Copy, Debug)]
pub(super) struct Modes {
    bits: u32,
}

impl Default for Modes {
    /// Power-on state: auto-wrap, cursor shown, alternate scroll and urgency hints on.
    fn default() -> Self {
        let on = [
            Mode::LineWrap,
            Mode::ShowCursor,
            Mode::AlternateScroll,
            Mode::UrgencyHints,
        ];
        Modes {
            bits: on.iter().fold(0, |bits, mode| bits | mode.bit()),
        }
    }
}

impl Modes {
    pub(super) fn get(self, mode: Mode) -> bool {
        self.bits & mode.bit() != 0
    }

    pub(super) fn set(&mut self, mode: Mode, on: bool) {
        if on {
            self.bits |= mode.bit();
        } else {
            self.bits &= !mode.bit();
        }
    }

    /// Turns `on` for `mode` after clearing the rest of its exclusive group.
    fn set_exclusive(&mut self, group: &[Mode], mode: Mode, on: bool) {
        if on {
            for other in group {
                self.set(*other, false);
            }
        }
        self.set(mode, on);
    }
}

/// The shape of the cursor.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum CursorShape {
    /// A filled cell.
    #[default]
    Block,
    /// A line under the cell.
    Underline,
    /// A vertical bar at the left of the cell.
    Beam,
    /// An outlined cell; embedder-side only (an unfocused window, or the configured default).
    HollowBlock,
    /// Not drawn; reported while `?25` is reset.
    Hidden,
}

/// The cursor shape and whether it blinks.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct CursorStyle {
    /// The shape.
    pub shape: CursorShape,
    /// Whether it blinks.
    pub blinking: bool,
}

bitflags! {
    /// The flags of the kitty keyboard protocol.
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
    pub struct KeyboardFlags: u8 {
        /// Report keys that would be ambiguous as escape codes.
        const DISAMBIGUATE_ESC_CODES = 1;
        /// Report press, repeat and release.
        const REPORT_EVENT_TYPES = 2;
        /// Report shifted and base-layout keys.
        const REPORT_ALTERNATE_KEYS = 4;
        /// Report every key as an escape code.
        const REPORT_ALL_KEYS_AS_ESC = 8;
        /// Report the text a key produces.
        const REPORT_ASSOCIATED_TEXT = 16;
    }
}

/// The pushed flag sets of one screen and the set in force.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FlagStack {
    flags: [KeyboardFlags; KEYBOARD_STACK_MAX],
    len: u8,
    live: KeyboardFlags,
}

impl FlagStack {
    pub(super) fn live(&self) -> KeyboardFlags {
        self.live
    }

    /// `CSI > f u`: pushes `f`; a full stack drops its oldest entry.
    pub(super) fn push(&mut self, flags: u16) {
        let flags = KeyboardFlags::from_bits_truncate((flags & 0x1F) as u8);
        let len = usize::from(self.len);
        if len == KEYBOARD_STACK_MAX {
            self.flags.copy_within(1.., 0);
        }
        let top = len.min(KEYBOARD_STACK_MAX - 1);
        if let Some(slot) = self.flags.get_mut(top) {
            *slot = flags;
        }
        self.len = (top + 1) as u8;
        self.live = flags;
    }

    /// `CSI < n u`: pops `n` entries; `n` of the depth or more empties the stack.
    pub(super) fn pop(&mut self, count: u16) {
        self.len = if usize::from(count) >= usize::from(self.len) {
            0
        } else {
            self.len - count as u8
        };
        self.live = usize::from(self.len)
            .checked_sub(1)
            .and_then(|top| self.flags.get(top))
            .copied()
            .unwrap_or_default();
    }

    /// `CSI = f ; m u`: edits the flags in force without touching the stack.
    pub(super) fn set(&mut self, flags: u16, how: u16) {
        let flags = KeyboardFlags::from_bits_truncate((flags & 0x1F) as u8);
        self.live = match how {
            2 => self.live | flags,
            3 => self.live & !flags,
            _ => flags,
        };
    }
}

/// The kitty flag stacks of the two screens; the active one answers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct KeyboardStacks {
    pub(super) active: FlagStack,
    inactive: FlagStack,
}

impl KeyboardStacks {
    /// Called on every real switch between the screens.
    pub(super) fn swap(&mut self) {
        std::mem::swap(&mut self.active, &mut self.inactive);
    }
}

/// The window title and the titles pushed with `CSI 22 t`.
#[derive(Debug, Default)]
pub(super) struct TitleStack {
    pub(super) title: Option<String>,
    stack: Vec<Option<String>>,
}

impl TitleStack {
    pub(super) fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub(super) fn depth(&self) -> usize {
        self.stack.len()
    }

    /// `CSI 22 t`: keeps a copy of the title; at the cap the oldest entry goes.
    pub(super) fn push(&mut self) {
        if self.stack.len() >= TITLE_STACK_MAX {
            self.stack.remove(0);
        }
        self.stack.push(self.title.clone());
    }

    /// `CSI 23 t`: restores the last pushed title. `None` when nothing was pushed, `Some(None)`
    /// when the pushed state was "no title".
    pub(super) fn pop(&mut self) -> Option<Option<&str>> {
        self.title = self.stack.pop()?;
        Some(self.title.as_deref())
    }
}

impl State {
    /// The cursor style a program set, else the configured default.
    pub(super) fn stored_cursor_style(&self) -> CursorStyle {
        self.cursor_style
            .unwrap_or(self.config.default_cursor_style)
    }

    /// [`Terminal::cursor_style`](super::Terminal::cursor_style).
    pub(super) fn cursor_style(&self) -> CursorStyle {
        let mut style = self.stored_cursor_style();
        if !self.modes.get(Mode::ShowCursor) {
            style.shape = CursorShape::Hidden;
        }
        style
    }

    /// [`Terminal::mode`](super::Terminal::mode).
    pub(super) fn mode(&self, mode: Mode) -> bool {
        match mode {
            Mode::AltScreen47 | Mode::AltScreen1047 | Mode::AltScreen => self.grid.alt_active(),
            Mode::CursorBlink => self.stored_cursor_style().blinking,
            Mode::DecCoLm | Mode::SaveCursor1048 => false,
            _ => self.modes.get(mode),
        }
    }

    /// What DECRQM answers for `mode`.
    pub(super) fn mode_state(&self, mode: Mode) -> ModeState {
        if let Some(fixed) = mode.inert_state() {
            return fixed;
        }
        if mode == Mode::SaveCursor1048 {
            return ModeState::Reset;
        }
        if self.mode(mode) {
            ModeState::Set
        } else {
            ModeState::Reset
        }
    }
}

impl Handler<'_> {
    /// `CSI Pm h` and `CSI Pm l`, with or without `?`: every parameter is a mode number; an
    /// unknown one is counted.
    pub(super) fn set_modes(&mut self, args: &mut Args<'_>, private: bool, on: bool) {
        let before = self.state.cursor_style();
        while let Some(number) = args.next_raw() {
            let mode = if private {
                Mode::from_private(number)
            } else {
                Mode::from_ansi(number)
            };
            match mode {
                Some(mode) => self.apply_mode(mode, on),
                None => self.unhandled(),
            }
        }
        self.note_cursor_style(before);
    }

    /// Pushes `CursorStyleChanged` when the cursor style differs from `before`.
    pub(super) fn note_cursor_style(&mut self, before: CursorStyle) {
        if self.state.cursor_style() != before {
            self.out.push(VtEvent::CursorStyleChanged);
        }
    }

    fn apply_mode(&mut self, mode: Mode, on: bool) {
        match mode {
            Mode::DecCoLm => self.column_mode(),
            Mode::Origin => {
                self.state.modes.set(mode, on);
                let origin = self.origin();
                self.state.grid.screen_mut().goto_origin(0, 0, origin);
            }
            Mode::MouseX10 | Mode::MouseClick | Mode::MouseDrag | Mode::MouseMotion => {
                self.state.modes.set_exclusive(&MOUSE_PROTOCOLS, mode, on);
            }
            Mode::Utf8Mouse | Mode::SgrMouse | Mode::UrxvtMouse => {
                self.state.modes.set_exclusive(&MOUSE_ENCODINGS, mode, on);
            }
            Mode::CursorBlink => {
                let mut style = self.state.stored_cursor_style();
                style.blinking = on;
                self.state.cursor_style = Some(style);
            }
            Mode::AltScreen47 => {
                if on {
                    self.enter_alt(false);
                } else {
                    self.leave_alt();
                }
            }
            Mode::AltScreen1047 => {
                if on {
                    self.enter_alt(false);
                } else if self.state.grid.alt_active() {
                    self.state.grid.wipe();
                    self.leave_alt();
                }
            }
            Mode::AltScreen => {
                if on {
                    if !self.state.grid.alt_active() {
                        self.enter_alt(true);
                        self.state.grid.wipe();
                    }
                } else if self.state.grid.alt_active() {
                    self.leave_alt();
                    self.state.restore_cursor();
                }
            }
            Mode::SaveCursor1048 => {
                if on {
                    self.state.save_cursor();
                } else {
                    self.state.restore_cursor();
                }
            }
            Mode::GraphemeClusters => {
                self.state.modes.set(mode, on);
                if !on {
                    self.state.carry = None;
                }
            }
            _ => self.state.modes.set(mode, on),
        }
    }

    /// `CursorOrigin::Region` while DECOM is set.
    pub(super) fn origin(&self) -> CursorOrigin {
        if self.state.modes.get(Mode::Origin) {
            CursorOrigin::Region
        } else {
            CursorOrigin::Screen
        }
    }

    /// DECCOLM, set or reset: clears the screen, restores the full region and homes the
    /// cursor. The width never changes.
    fn column_mode(&mut self) {
        self.state.invalidate(Invalidation::EraseScreen);
        let screen = self.state.grid.screen_mut();
        let rows = screen.rows();
        screen.set_region(0, rows, CursorOrigin::Screen);
        screen.reset_screen_rows(0, rows);
    }

    /// Shows the alternate screen; saves the cursor first when `save`. Does nothing if it is
    /// shown already.
    fn enter_alt(&mut self, save: bool) {
        if self.state.grid.alt_active() {
            return;
        }
        self.state.invalidate(Invalidation::SwapAlt);
        if save {
            self.state.save_cursor();
        }
        self.state.grid.enter_alt();
        self.state.keyboard.swap();
    }

    /// Shows the primary screen. Does nothing if it is shown already.
    fn leave_alt(&mut self) {
        if !self.state.grid.alt_active() {
            return;
        }
        self.state.invalidate(Invalidation::SwapAlt);
        self.state.grid.leave_alt();
        self.state.keyboard.swap();
    }

    /// `CSI Ps SP q`.
    pub(super) fn set_cursor_style(&mut self, args: &mut Args<'_>) {
        let before = self.state.cursor_style();
        let style = match args.next_raw().unwrap_or(0) {
            0 => None,
            selector @ 1..=6 => Some(CursorStyle {
                shape: match (selector - 1) / 2 {
                    0 => CursorShape::Block,
                    1 => CursorShape::Underline,
                    _ => CursorShape::Beam,
                },
                blinking: selector % 2 == 1,
            }),
            _ => {
                self.unhandled();
                return;
            }
        };
        self.state.cursor_style = style;
        self.note_cursor_style(before);
    }

    /// RIS: back to power-on state. Row ids, the line count, the config, the style table and
    /// the grapheme arena stay.
    pub(super) fn full_reset(&mut self) {
        let before = self.state.cursor_style();
        self.state.invalidate(Invalidation::Reset);
        let state = &mut *self.state;
        state.grid.reset();
        // No cell or pen holds an extras id after the grid reset.
        state.interner.hyperlinks.clear();
        state.interner.extras.clear();
        state.modes = Modes::default();
        state.keyboard = KeyboardStacks::default();
        state.colors.reset_all();
        state.title = TitleStack::default();
        state.cursor_style = None;
        state.modify_other_keys = 0;
        state.shifts = Shifts::default();
        state.preceding_char = None;
        state.carry = None;
        state.dcs_query = None;
        state.dcs_payload.clear();
        self.out.push(VtEvent::ScreenCleared);
        self.note_cursor_style(before);
    }

    /// DECSTR: resets modes and the pen, keeps the screen, title, colours, designations and
    /// shifts, cursor style, keyboard state, application cursor keys and mouse modes.
    pub(super) fn soft_reset(&mut self) {
        let before = self.state.cursor_style();
        let state = &mut *self.state;
        state.modes.set(Mode::Origin, false);
        state.modes.set(Mode::Insert, false);
        state.modes.set(Mode::LineWrap, true);
        state.modes.set(Mode::ShowCursor, true);
        state.dcs_query = None;
        state.dcs_payload.clear();
        let screen = state.grid.screen_mut();
        let rows = screen.rows();
        screen.set_region(0, rows, CursorOrigin::Screen);
        screen.set_template(Cell::EMPTY, &mut state.interner);
        screen.goto(0, 0);
        state.save_cursor();
        self.note_cursor_style(before);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::terminal_tests::Session;

    /// Power-on DECRQM answer of a private mode number.
    fn power_on_state(mode: Mode) -> u8 {
        let on = [
            Mode::LineWrap,
            Mode::ShowCursor,
            Mode::AlternateScroll,
            Mode::UrgencyHints,
        ];
        match mode.inert_state() {
            Some(fixed) => fixed as u8,
            None if on.contains(&mode) => 1,
            None => 2,
        }
    }

    fn query(session: &mut Session, private: bool, number: u16) -> String {
        let marker = if private { "?" } else { "" };
        session.feed(format!("\x1b[{marker}{number}$p").as_bytes());
        session.replies()
    }

    #[test]
    fn decrqm_walk_at_power_on_and_after_set() {
        for &(number, mode) in PRIVATE_MODES {
            let mut session = Session::new(10, 4);
            let expected = format!("\x1b[?{number};{}$y", power_on_state(mode));
            assert_eq!(query(&mut session, true, number), expected, "?{number}");

            session.feed(format!("\x1b[?{number}h").as_bytes());
            let set = match mode.inert_state() {
                Some(fixed) => fixed as u8,
                None if mode == Mode::SaveCursor1048 => 2,
                None => 1,
            };
            let expected = format!("\x1b[?{number};{set}$y");
            assert_eq!(query(&mut session, true, number), expected, "?{number} h");
        }
        for &(number, mode) in ANSI_MODES {
            let mut session = Session::new(10, 4);
            let expected = format!("\x1b[{number};{}$y", power_on_state(mode));
            assert_eq!(query(&mut session, false, number), expected, "{number}");
            session.feed(format!("\x1b[{number}h").as_bytes());
            let set = mode.inert_state().map_or(1, |fixed| fixed as u8);
            let expected = format!("\x1b[{number};{set}$y");
            assert_eq!(query(&mut session, false, number), expected, "{number} h");
        }
    }

    #[test]
    fn inert_modes_never_answer_set() {
        for &(number, mode) in PRIVATE_MODES.iter().chain(ANSI_MODES) {
            if mode.inert_state().is_none() {
                continue;
            }
            let mut session = Session::new(10, 4);
            let private = PRIVATE_MODES.contains(&(number, mode));
            session.feed(format!("\x1b[{}{number}h", if private { "?" } else { "" }).as_bytes());
            let reply = query(&mut session, private, number);
            assert!(!reply.contains(";1$y"), "{mode:?}: {reply:?}");
            assert!(!reply.contains(";3$y"), "{mode:?}: {reply:?}");
        }
    }

    #[test]
    fn unknown_numbers_answer_not_supported() {
        let mut session = Session::new(10, 4);
        assert_eq!(query(&mut session, true, 9999), "\x1b[?9999;0$y");
        assert_eq!(query(&mut session, false, 99), "\x1b[99;0$y");
        assert_eq!(query(&mut session, true, 0), "\x1b[?0;0$y");
    }

    #[test]
    fn numbers_are_unique_and_round_trip() {
        for (index, &(number, mode)) in PRIVATE_MODES.iter().enumerate() {
            assert_eq!(Mode::from_private(number), Some(mode));
            assert!(
                PRIVATE_MODES[index + 1..]
                    .iter()
                    .all(|(n, m)| *n != number && *m != mode)
            );
        }
        for &(number, mode) in ANSI_MODES {
            assert_eq!(Mode::from_ansi(number), Some(mode));
        }
        assert_eq!(Mode::from_private(4), None);
        assert_eq!(Mode::from_ansi(25), None);
    }
}
