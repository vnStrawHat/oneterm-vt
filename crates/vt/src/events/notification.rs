//! The desktop notification a program asked for, as [`VtEvent::Notification`] carries it.
//!
//! [`VtEvent::Notification`]: crate::VtEvent::Notification

use super::{ByteSpan, EventBatch, ParamSpans, StrSpan};

/// A desktop notification a program asked for with OSC 9, OSC 777 or OSC 99.
///
/// The engine gathered and checked what the program sent; everything here is for the embedder
/// to act on or to ignore. OSC 9 and OSC 777 fill the title and the body only; the other
/// fields then hold what OSC 99 means by a key that was not sent. Text is the program's:
/// treat it as untrusted. Get it with [`EventBatch::notification`] and read its spans and
/// lists back through the same batch ([`EventBatch::str`], [`EventBatch::bytes`],
/// [`EventBatch::strs`]).
///
/// [`EventBatch::notification`]: crate::EventBatch::notification
///
/// [`EventBatch::str`]: crate::EventBatch::str
/// [`EventBatch::bytes`]: crate::EventBatch::bytes
/// [`EventBatch::strs`]: crate::EventBatch::strs
#[non_exhaustive]
#[derive(PartialEq, Eq, Debug)]
pub struct Notification {
    /// The id the program gave it (OSC 99 `i`), empty when it gave none. A notification with
    /// the id of one still shown replaces it; without an id nothing is ever replaced. Reports
    /// name this id.
    pub id: StrSpan,
    /// The title. Empty for OSC 9; never empty for OSC 99.
    pub title: StrSpan,
    /// The message; empty when there is only a title.
    pub body: StrSpan,
    /// The name of the sending application (`f`), for filtering and for finding an icon.
    pub app_name: StrSpan,
    /// The kinds of notification this is (`t`), for filtering.
    pub kinds: ParamSpans,
    /// Icon names to try in order (`n`): an application name or a symbol such as `error`,
    /// `warning`, `info`, `question`, `help`.
    pub icon_names: ParamSpans,
    /// The cache key of the icon data (`g`). With `icon`, store the data under it; without,
    /// show the data stored earlier. Empty when none was sent.
    pub icon_id: StrSpan,
    /// The icon image as sent (`p=icon`): a PNG, JPEG or GIF file, not decoded. Empty when
    /// none was sent. Prefer an icon found by name.
    pub icon: ByteSpan,
    /// The labels of the buttons to show (`p=buttons`), first button first.
    pub buttons: ParamSpans,
    /// The sound to play (`s`): `system`, `silent`, `error`, `warning`, `info`, `question`, or
    /// a name of the platform. Empty when none was sent, which means `system`.
    pub sound: StrSpan,
    /// How urgent it is (`u`); `None` when the program did not say, which means normal.
    pub urgency: Option<NotificationUrgency>,
    /// When the program wants it shown (`o`). The engine does not know your focus: applying
    /// this is yours.
    pub occasion: NotificationOccasion,
    /// When it should close by itself (`w`).
    pub expiry: NotificationExpiry,
    /// Whether activating it should focus the window it came from (`a=focus`, the default).
    pub focus_on_activation: bool,
    /// Whether the program wants to hear that it was activated, or which button was pressed
    /// (`a=report`): write [`NotificationReport::Activated`] or
    /// [`NotificationReport::Button`].
    ///
    /// [`NotificationReport::Activated`]: crate::input::NotificationReport::Activated
    /// [`NotificationReport::Button`]: crate::input::NotificationReport::Button
    pub report_activation: bool,
    /// Whether the program wants to hear that it was closed (`c=1`): write
    /// [`NotificationReport::Closed`], or `CloseUntracked` if your platform cannot tell.
    ///
    /// [`NotificationReport::Closed`]: crate::input::NotificationReport::Closed
    pub report_close: bool,
}

impl Notification {
    /// A notification that is a title and a body, every other field as OSC 99 defines a key
    /// that was not sent.
    pub(crate) fn text(batch: &mut EventBatch, title: &str, body: &str) -> Notification {
        Notification {
            id: StrSpan::EMPTY,
            title: batch.text_span(title),
            body: batch.text_span(body),
            app_name: StrSpan::EMPTY,
            kinds: ParamSpans::EMPTY,
            icon_names: ParamSpans::EMPTY,
            icon_id: StrSpan::EMPTY,
            icon: ByteSpan::EMPTY,
            buttons: ParamSpans::EMPTY,
            sound: StrSpan::EMPTY,
            urgency: None,
            occasion: NotificationOccasion::Always,
            expiry: NotificationExpiry::Platform,
            focus_on_activation: true,
            report_activation: false,
            report_close: false,
        }
    }
}

/// How urgent a notification is (OSC 99 `u`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NotificationUrgency {
    /// `0`.
    Low,
    /// `1`.
    Normal,
    /// `2`.
    Critical,
}

/// When a program wants its notification shown (OSC 99 `o`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum NotificationOccasion {
    /// Whatever the window's state.
    #[default]
    Always,
    /// Only while the window it came from does not have the keyboard focus.
    Unfocused,
    /// Only while that window is unfocused and not visible (an inactive tab, a minimised or
    /// covered window).
    Invisible,
}

/// When a notification should close by itself (OSC 99 `w`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum NotificationExpiry {
    /// As the platform decides (`-1`, or no `w`).
    #[default]
    Platform,
    /// Never (`0`); best effort.
    Never,
    /// After this many milliseconds.
    After(u32),
}
