//! The modes a renderer or an input encoder reads, copied out of the terminal.

use crate::terminal::KeyboardFlags;

/// Which mouse events a program asked for.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MouseReporting {
    /// `?9`: button presses only.
    X10,
    /// `?1000`: presses and releases.
    Normal,
    /// `?1002`: presses, releases and motion while a button is held.
    ButtonEvent,
    /// `?1003`: all motion.
    AnyEvent,
}

/// How mouse reports are written.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum MouseEncoding {
    /// Three bytes after `CSI M`, each offset by 32.
    #[default]
    Default,
    /// `?1005`: like `Default`, with UTF-8 coordinates.
    Utf8,
    /// `?1006`: `CSI < b ; x ; y M` or `m`.
    Sgr,
    /// `?1015`: `CSI b ; x ; y M`.
    Urxvt,
}

/// Mouse reporting as a program set it: what to report and how to write it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MouseProtocol {
    /// Which events are reported.
    pub reporting: MouseReporting,
    /// How a report is encoded.
    pub encoding: MouseEncoding,
}

/// The terminal modes outside the grid, as of one call.
///
/// Outside the crate, start from [`ModeSnapshot::default`] or
/// [`Terminal::mode_snapshot`](crate::Terminal::mode_snapshot) and assign fields: the struct
/// is `#[non_exhaustive]`, so a new field is not a breaking change.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ModeSnapshot {
    /// The alternate screen is shown.
    pub alt_screen: bool,
    /// `?1`: cursor keys send application sequences.
    pub app_cursor: bool,
    /// `ESC =`: the keypad sends application sequences.
    pub app_keypad: bool,
    /// `?2004`: pasted text is bracketed.
    pub bracketed_paste: bool,
    /// `4`: printing inserts instead of overwriting.
    pub insert: bool,
    /// `?25`: the cursor is shown.
    pub show_cursor: bool,
    /// `?1007`: the wheel sends arrow keys on the alternate screen.
    pub alternate_scroll: bool,
    /// `?5`: swap the default foreground and background. No cell changes; the renderer swaps.
    pub reverse_video: bool,
    /// The mouse protocol, if reporting is on.
    pub mouse: Option<MouseProtocol>,
    /// The kitty keyboard flags in force on the shown screen.
    pub keyboard_flags: KeyboardFlags,
    /// The `modifyOtherKeys` level, `0..=2`.
    pub modify_other_keys: u8,
}

impl Default for ModeSnapshot {
    /// The power-on state: the cursor is shown and alternate scroll is on, the rest is off.
    fn default() -> ModeSnapshot {
        ModeSnapshot {
            alt_screen: false,
            app_cursor: false,
            app_keypad: false,
            bracketed_paste: false,
            insert: false,
            show_cursor: true,
            alternate_scroll: true,
            reverse_video: false,
            mouse: None,
            keyboard_flags: KeyboardFlags::empty(),
            modify_other_keys: 0,
        }
    }
}
