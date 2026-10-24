//! What `Terminal::feed` reports: events, the batch that holds them, and per-call counters.

mod batch;

#[cfg(test)]
mod event_tests;

pub use batch::{ByteSpan, EventBatch, ParamSpans, StrSpan};
#[cfg(test)]
pub(crate) use batch::{EVENT_ARENA_SOFT, EVENT_LIST_SOFT};

use crate::grid::RowsScrolled;
use crate::intern::GraphicId;
use crate::parser::StringTerm;
use crate::terminal::ColorKey;

/// One thing that happened while a `feed` call parsed output.
///
/// Events arrive in byte order in an [`EventBatch`]; payloads are spans read back through the
/// batch. The embedder owns every policy: the engine never writes a reply, sets a clipboard or
/// shows a title by itself. Under [`OscRoute::BuiltinAndForward`](crate::OscRoute) the typed
/// event comes first, then [`VtEvent::Osc`]. Not `Clone`, so an event cannot outlive its batch
/// unnoticed.
#[non_exhaustive]
#[derive(PartialEq, Eq, Debug)]
pub enum VtEvent {
    /// The feed made at least one dispatch call; a hint to redraw, not damage. At most once,
    /// last.
    Repaint,
    /// `BEL` outside a string.
    Bell,
    /// The window title changed: OSC 0 or 2 with text, or `CSI 23 t` popping a title.
    Title(StrSpan),
    /// The title was cleared: OSC 0 or 2 with empty text, or `CSI 23 t` popping "no title".
    TitleReset,
    /// OSC 1: the icon name, trimmed, possibly empty.
    IconName(StrSpan),
    /// An answer the program asked for. Write the bytes to it verbatim, in order.
    Reply(ByteSpan),
    /// OSC 4, 10, 11, 12, 17 or 19 with the value `?`. Answer with
    /// [`ColorKey::color_reply`] and the same `terminator`.
    ColorQuery {
        /// The colour asked about.
        key: ColorKey,
        /// How the query ended; echo it in the reply.
        terminator: StringTerm,
    },
    /// OSC 52 store: decoded, valid UTF-8 text. Whether to honour it is the embedder's policy.
    ClipboardStore {
        /// Which selection.
        selection: ClipboardKind,
        /// The text.
        text: StrSpan,
    },
    /// OSC 52 with the payload `?`: the program asks for the clipboard. Answering is policy.
    ClipboardLoad {
        /// Which selection.
        selection: ClipboardKind,
    },
    /// ED 2, ED 3 (even with empty history) or RIS. Pushed before the scroll events the same
    /// operation produces.
    ScreenCleared,
    /// Content moved to other row ids (region scrolls, wraps included).
    RowsScrolled(RowsScrolled),
    /// Rows left the oldest end of the history; `oldest` is the new oldest row id.
    RowsTrimmed {
        /// The new oldest live row of the screen.
        oldest: crate::grid::RowId,
    },
    /// OSC 7: the working directory.
    Cwd {
        /// The authority of the `file://` URL, or empty.
        host: StrSpan,
        /// The percent-decoded path, or the text as sent when there was no `file://` prefix.
        path: StrSpan,
    },
    /// OSC 9 (other than `9;4`): a desktop notification.
    Notification {
        /// Empty for OSC 9.
        title: StrSpan,
        /// The message.
        body: StrSpan,
    },
    /// OSC 9;4: taskbar progress.
    Progress(Progress),
    /// OSC 22: the mouse pointer shape name.
    Pointer(StrSpan),
    /// OSC 133: a shell-integration mark.
    ShellMark(ShellMark),
    /// The image is gone from the grid (its rows were scrolled out, erased or reflowed away,
    /// it was evicted, covered by a newer image, or its pixels were dropped from the queue).
    /// Free the texture you made from the [`GraphicData`](crate::GraphicData) with this id;
    /// the id is never used again. Delivered once per image, before [`VtEvent::Repaint`].
    GraphicReleased(GraphicId),
    /// `Terminal::cursor_style` differs from its value before the sequence that just ran.
    CursorStyleChanged,
    /// An OSC the embedder routed with `Forward` or `BuiltinAndForward`. One span per
    /// `;`-separated parameter; parameter 0 is the code text.
    Osc {
        /// The OSC number.
        code: u32,
        /// The parameters, read back with [`EventBatch::params`].
        params: ParamSpans,
        /// How the string ended; echo it in any reply.
        terminator: StringTerm,
        /// Bytes past the size cap were dropped.
        truncated: bool,
    },
}

/// The selection an OSC 52 names.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ClipboardKind {
    /// `c`, or no letter: the system clipboard.
    Clipboard,
    /// `p`: the primary selection.
    Primary,
    /// `s`: the secondary selection.
    Secondary,
}

/// The state OSC 9;4 reports. Percentages are `0..=100`.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Progress {
    /// No progress is shown.
    Remove,
    /// A determinate progress of the given percent.
    Set(u8),
    /// An error state at the given percent.
    Error(u8),
    /// Progress with no known amount.
    Indeterminate,
    /// A paused state at the given percent.
    Paused(u8),
}

/// A shell-integration mark (OSC 133).
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ShellMark {
    /// `A`: the prompt starts.
    PromptStart,
    /// `B`: the prompt ends and input starts.
    PromptEnd,
    /// `C`: the command's output starts.
    OutputStart,
    /// `D`: the command finished.
    OutputEnd {
        /// The exit status, when the mark carried a number.
        exit_code: Option<i32>,
    },
}

/// Counters for one `feed` call. Every field counts that call only, never the session, and
/// saturates. The meaning of each field is part of the crate's API.
///
/// Read-only for embedders (it is built only through `Default`), so a new counter is a minor
/// version bump.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct FeedStats {
    /// Bytes in the call.
    pub bytes: usize,
    /// Rows that left the top of a screen into its history: line feeds, wraps, IND, NEL, SU,
    /// DL at row 0, ED 2 on the primary screen. On the alternate screen, which keeps no
    /// history, they count although the rows are dropped at once.
    pub rows_scrolled: u32,
    /// Sequences parsed but not acted on: an unknown key, a value the engine does not
    /// implement, a rejected OSC, a gated query, a payload past its ceiling.
    pub unhandled_sequences: u32,
    /// Sequences routed to a handler whose payload was unusable: an OSC 52 store that is not
    /// canonical base64 or not UTF-8.
    pub malformed_sequences: u32,
    /// OSC strings that arrived with bytes past the size cap dropped, also those not routed.
    pub truncated_osc: u32,
    /// DCS strings ended by `CAN`/`SUB` or by the payload cap, of any kind.
    pub aborted_dcs: u32,
    /// OSC 8 links dropped because the hyperlink or extras table was full.
    pub hyperlink_table_exhausted: u32,
    /// SGR states that fell back to the default style because the style table was full.
    pub style_table_exhausted: u32,
    /// Clusters cut at 16 scalars.
    pub grapheme_truncated: u32,
    /// Clusters stored as a blank because the grapheme arena was full.
    pub grapheme_table_exhausted: u32,
    /// Clusters over 32 scalars under `?2027`, whose continuation across chunks cannot be
    /// joined; counted once per placed cluster.
    pub dropped_cluster_carries: u32,
    /// Wide glyphs that could not be placed (a one-column screen, or the last column with
    /// autowrap off).
    pub wide_glyphs_dropped: u32,
    /// Images whose pixels were dropped before `Terminal::take_graphics` because more than
    /// 128 MiB were waiting; each also gets a `GraphicReleased`.
    pub graphics_dropped: u32,
}
