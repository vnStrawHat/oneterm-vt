//! Desktop notifications sent with OSC 99, the kitty desktop-notification protocol.
//!
//! The engine does the protocol: it reads every key, joins the chunks of a notification,
//! decodes base64, bounds what a program can make it hold, answers the capability query from
//! what the embedder declared in [`NotificationSupport`], and reports a finished notification
//! as one [`VtEvent::Notification`]. Showing it, applying its occasion, focusing a window,
//! playing a sound, finding and caching icons and telling the program what the user did
//! ([`NotificationReport`](crate::input::NotificationReport)) are the embedder's.

use std::borrow::Cow;

use crate::events::{
    Notification, NotificationExpiry, NotificationOccasion, NotificationSource,
    NotificationUrgency, VtEvent, base64_value,
};
use crate::input::clean_id;
use crate::parser::{OscParams, StringTerm};

use super::dispatch::Handler;

/// Notifications that may wait for their last chunk at once; one more drops the one that
/// waited longest.
pub(super) const PENDING_NOTIFICATIONS_MAX: usize = 8;
/// Bytes kept of a title, of a body and of the button labels together; what a chunk adds past
/// it is dropped.
pub(super) const NOTIFICATION_TEXT_MAX: usize = 4096;
/// Longest notification id or icon id, counted after the characters an id may not hold are
/// removed.
pub(super) const NOTIFICATION_ID_MAX: usize = 128;
/// Longest application name, sound name, notification kind or icon name, decoded.
pub(super) const NOTIFICATION_NAME_MAX: usize = 256;
/// Notification kinds, and icon names, one notification holds.
pub(super) const NOTIFICATION_LIST_MAX: usize = 8;
/// Largest icon image; a larger one is dropped whole.
pub(super) const NOTIFICATION_ICON_MAX: usize = 1024 * 1024;

/// What the embedder can do with an OSC 99 notification: the answer to the query a program
/// sends before it uses the protocol (`OSC 99 ; i=id : p=? ; ST`).
///
/// Everything is off by default, which answers "titles and bodies, shown always": declare
/// only what you implement, because programs rely on the answer. The declaration changes the
/// answer and nothing else: every key a program sends reaches you in the
/// [`Notification`] either way. Build it with the struct-update form,
/// `NotificationSupport { urgency: true, ..NotificationSupport::default() }`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct NotificationSupport {
    /// `a=focus`: you focus the window a notification came from when the user activates it.
    pub focus: bool,
    /// `a=report`: you write [`NotificationReport::Activated`] and
    /// [`NotificationReport::Button`] when the notification asked for them.
    ///
    /// [`NotificationReport::Activated`]: crate::input::NotificationReport::Activated
    /// [`NotificationReport::Button`]: crate::input::NotificationReport::Button
    pub report: bool,
    /// `c=1`: you write [`NotificationReport::Closed`] (or `CloseUntracked`) when the
    /// notification asked for it.
    ///
    /// [`NotificationReport::Closed`]: crate::input::NotificationReport::Closed
    pub close_events: bool,
    /// `o=unfocused`: you can hold a notification back while its window has the focus.
    pub unfocused: bool,
    /// `o=invisible`: you can hold a notification back while its window is visible.
    pub invisible: bool,
    /// `p=icon`: you show icons, by name (`icon_names`) and from data (`icon`, cached under
    /// `icon_id`).
    pub icons: bool,
    /// `p=buttons`: you show buttons.
    pub buttons: bool,
    /// `p=close`: you close a shown notification on
    /// [`VtEvent::NotificationClose`](crate::VtEvent::NotificationClose).
    pub close: bool,
    /// `p=alive`: you answer
    /// [`VtEvent::NotificationAliveQuery`](crate::VtEvent::NotificationAliveQuery).
    pub alive: bool,
    /// `s`: the sound names you can play, from `system`, `silent`, `error`, `warn`,
    /// `warning`, `info`, `question` and the names of your platform. A name with a character
    /// outside `a-z A-Z 0-9 _ - .` is dropped when the terminal is built.
    pub sounds: Vec<Cow<'static, str>>,
    /// `u`: you treat low, normal and critical notifications differently.
    pub urgency: bool,
    /// `w`: you close a notification after the time it asked for.
    pub expiry: bool,
}

impl NotificationSupport {
    /// The same declaration without the sound names that cannot go into a reply.
    pub(super) fn cleaned(mut self) -> NotificationSupport {
        self.sounds.retain(|name| {
            !name.is_empty()
                && name.len() <= NOTIFICATION_NAME_MAX
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
        });
        self
    }

    /// The payload of the answer to the query: one `key=value` per ability, `:` between them.
    /// A key that is absent says "not supported", as the protocol defines.
    fn answer(&self) -> String {
        let mut answer = String::new();
        let mut key = |name: &str, values: &[&str]| {
            if values.is_empty() {
                return;
            }
            if !answer.is_empty() {
                answer.push(':');
            }
            answer.push_str(name);
            answer.push('=');
            answer.push_str(&values.join(","));
        };
        let listed = |pairs: &[(bool, &'static str)]| -> Vec<&'static str> {
            pairs
                .iter()
                .filter(|(supported, _)| *supported)
                .map(|(_, name)| *name)
                .collect()
        };
        key(
            "a",
            &listed(&[(self.focus, "focus"), (self.report, "report")]),
        );
        key("c", &listed(&[(self.close_events, "1")]));
        key(
            "o",
            &listed(&[
                (true, "always"),
                (self.unfocused, "unfocused"),
                (self.invisible, "invisible"),
            ]),
        );
        key(
            "p",
            &listed(&[
                (true, "title"),
                (true, "body"),
                (true, "?"),
                (self.close, "close"),
                (self.icons, "icon"),
                (self.alive, "alive"),
                (self.buttons, "buttons"),
            ]),
        );
        let sounds: Vec<&str> = self.sounds.iter().map(Cow::as_ref).collect();
        key("s", &sounds);
        key("u", &listed(&[(self.urgency, "0,1,2")]));
        key("w", &listed(&[(self.expiry, "1")]));
        answer
    }
}

/// One payload of a notification, gathered chunk by chunk. Bytes, not a string: a chunk may
/// end inside a character.
#[derive(Debug, Default)]
struct Part {
    bytes: Vec<u8>,
    /// The bits of the base64 group the last chunk ended inside.
    group: u32,
    /// How many characters gave those bits, `0..=3`.
    group_len: u8,
}

/// What a chunk did to a [`Part`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Added {
    /// All of it was kept.
    Whole,
    /// The part is at its cap; the rest was dropped.
    Cut,
    /// It was not base64; nothing was kept.
    NotBase64,
}

impl Part {
    fn add_plain(&mut self, bytes: &[u8], cap: usize) -> Added {
        self.bytes.extend_from_slice(bytes);
        self.enforce_cap(cap)
    }

    /// Adds a piece of a base64 stream. The stream may be cut anywhere, also inside a group
    /// of four characters, and `=` ends a group early, so both chunks encoded one by one and
    /// one encoding cut into chunks decode.
    fn add_base64(&mut self, encoded: &[u8], cap: usize) -> Added {
        let (mark, group, group_len) = (self.bytes.len(), self.group, self.group_len);
        for &byte in encoded {
            if byte == b'=' {
                self.end_group();
                continue;
            }
            let Some(value) = base64_value(byte) else {
                self.bytes.truncate(mark);
                (self.group, self.group_len) = (group, group_len);
                return Added::NotBase64;
            };
            self.group = (self.group << 6) | u32::from(value);
            self.group_len += 1;
            if self.group_len == 4 {
                let [_, first, second, third] = self.group.to_be_bytes();
                self.bytes.extend_from_slice(&[first, second, third]);
                (self.group, self.group_len) = (0, 0);
            }
            if self.bytes.len() > cap {
                // Nothing past the cap is kept, so the rest need not be read.
                break;
            }
        }
        self.enforce_cap(cap)
    }

    /// Ends the group in progress: two characters hold one byte, three hold two.
    fn end_group(&mut self) {
        match self.group_len {
            2 => self.bytes.push((self.group >> 4) as u8),
            3 => self
                .bytes
                .extend_from_slice(&[(self.group >> 10) as u8, (self.group >> 2) as u8]),
            _ => {}
        }
        (self.group, self.group_len) = (0, 0);
    }

    fn enforce_cap(&mut self, cap: usize) -> Added {
        if self.bytes.len() <= cap {
            return Added::Whole;
        }
        self.bytes.truncate(cap);
        Added::Cut
    }

    /// The gathered text. A character the cap or the last chunk cut is left out; other bytes
    /// that are not UTF-8 are replaced.
    fn text(&mut self) -> Cow<'_, str> {
        self.end_group();
        let whole = match std::str::from_utf8(&self.bytes) {
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            _ => self.bytes.len(),
        };
        String::from_utf8_lossy(self.bytes.get(..whole).unwrap_or_default())
    }
}

/// Why a metadata key was not applied.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum KeyError {
    /// A value the protocol does not define, or one past its cap.
    Unusable,
    /// A value that should be base64 UTF-8 and is not.
    Malformed,
}

/// What one OSC 99 left undone, for the counters.
#[derive(Clone, Copy, Debug, Default)]
struct Verdict {
    /// Something in the sequence was not acted on.
    ignored: bool,
    /// Something that had to be base64 was not.
    malformed: bool,
}

impl Verdict {
    fn note(&mut self, error: KeyError) {
        match error {
            KeyError::Unusable => self.ignored = true,
            KeyError::Malformed => self.malformed = true,
        }
    }
}

/// A notification whose last chunk (`d=1`, or no `d`) has not arrived.
#[derive(Debug)]
pub(super) struct PendingNotification {
    id: String,
    title: Part,
    body: Part,
    buttons: Part,
    icon: Part,
    /// The icon passed [`NOTIFICATION_ICON_MAX`]: it is gone and its later chunks are skipped.
    icon_refused: bool,
    app_name: String,
    kinds: Vec<String>,
    icon_names: Vec<String>,
    icon_id: String,
    sound: String,
    urgency: Option<NotificationUrgency>,
    occasion: NotificationOccasion,
    expiry: NotificationExpiry,
    focus: bool,
    report: bool,
    report_close: bool,
}

impl PendingNotification {
    /// A notification as the protocol defines one no key was sent for.
    fn new(id: String) -> PendingNotification {
        PendingNotification {
            id,
            title: Part::default(),
            body: Part::default(),
            buttons: Part::default(),
            icon: Part::default(),
            icon_refused: false,
            app_name: String::new(),
            kinds: Vec::new(),
            icon_names: Vec::new(),
            icon_id: String::new(),
            sound: String::new(),
            urgency: None,
            occasion: NotificationOccasion::Always,
            expiry: NotificationExpiry::Platform,
            focus: true,
            report: false,
            report_close: false,
        }
    }

    /// Applies the keys that describe the notification as a whole. Any chunk may carry them;
    /// a later chunk changes what an earlier one said.
    fn apply_keys(&mut self, metadata: &[u8], verdict: &mut Verdict) {
        for (key, value) in pairs(metadata) {
            let applied = match key {
                b'a' => self.apply_actions(value),
                b'c' => {
                    self.report_close = value != b"0";
                    Ok(())
                }
                b'f' => decode_name(value).map(|name| self.app_name = name),
                b'g' => clean_id_within_cap(value).map(|id| self.icon_id = id),
                b'n' => decode_name(value).and_then(|name| add_name(&mut self.icon_names, name)),
                b'o' => occasion(value).map(|occasion| self.occasion = occasion),
                b's' => decode_name(value).map(|name| self.sound = name),
                b't' => decode_name(value).and_then(|name| add_name(&mut self.kinds, name)),
                b'u' => urgency(value).map(|urgency| self.urgency = Some(urgency)),
                b'w' => expiry(value).map(|expiry| self.expiry = expiry),
                // `i`, `p`, `d` and `e` belong to the chunk; any other key is one the
                // protocol may define later.
                _ => Ok(()),
            };
            if let Err(error) = applied {
                verdict.note(error);
            }
        }
    }

    /// `a`: `focus` and `report`, each turned off by a leading `-`.
    fn apply_actions(&mut self, value: &[u8]) -> Result<(), KeyError> {
        let mut known = Ok(());
        for action in value.split(|byte| *byte == b',') {
            let (name, on) = match action.strip_prefix(b"-") {
                Some(name) => (name, false),
                None => (action, true),
            };
            match name {
                b"focus" => self.focus = on,
                b"report" => self.report = on,
                _ => known = Err(KeyError::Unusable),
            }
        }
        known
    }
}

/// The `key=value` pairs of a metadata section, split on `:`. A key is one byte; anything
/// else between two colons is skipped.
fn pairs(metadata: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    metadata
        .split(|byte| *byte == b':')
        .filter_map(|pair| match pair {
            [key, b'=', value @ ..] => Some((*key, value)),
            _ => None,
        })
}

/// An id without the characters an id may not hold; refused when what is left is too long.
fn clean_id_within_cap(id: &[u8]) -> Result<String, KeyError> {
    let clean = clean_id(id);
    if clean.len() > NOTIFICATION_ID_MAX {
        return Err(KeyError::Unusable);
    }
    Ok(clean)
}

/// A base64 UTF-8 name of at most [`NOTIFICATION_NAME_MAX`] bytes.
fn decode_name(value: &[u8]) -> Result<String, KeyError> {
    let mut part = Part::default();
    // One byte of room past the cap tells "at the cap" from "past it".
    match part.add_base64(value, NOTIFICATION_NAME_MAX + 1) {
        Added::Whole | Added::Cut => {}
        Added::NotBase64 => return Err(KeyError::Malformed),
    }
    part.end_group();
    if part.bytes.len() > NOTIFICATION_NAME_MAX {
        return Err(KeyError::Unusable);
    }
    String::from_utf8(part.bytes).map_err(|_| KeyError::Malformed)
}

/// Adds a name to a list that holds each name once; an empty name adds nothing.
fn add_name(list: &mut Vec<String>, name: String) -> Result<(), KeyError> {
    if name.is_empty() || list.contains(&name) {
        return Ok(());
    }
    if list.len() >= NOTIFICATION_LIST_MAX {
        return Err(KeyError::Unusable);
    }
    list.push(name);
    Ok(())
}

fn occasion(value: &[u8]) -> Result<NotificationOccasion, KeyError> {
    match value {
        b"always" => Ok(NotificationOccasion::Always),
        b"unfocused" => Ok(NotificationOccasion::Unfocused),
        b"invisible" => Ok(NotificationOccasion::Invisible),
        _ => Err(KeyError::Unusable),
    }
}

fn urgency(value: &[u8]) -> Result<NotificationUrgency, KeyError> {
    match value {
        b"0" => Ok(NotificationUrgency::Low),
        b"1" => Ok(NotificationUrgency::Normal),
        b"2" => Ok(NotificationUrgency::Critical),
        _ => Err(KeyError::Unusable),
    }
}

/// `w`: `-1` leaves it to the platform, `0` never expires, a larger number is milliseconds.
fn expiry(value: &[u8]) -> Result<NotificationExpiry, KeyError> {
    let milliseconds: i64 = std::str::from_utf8(value)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or(KeyError::Unusable)?;
    match milliseconds {
        -1 => Ok(NotificationExpiry::Platform),
        0 => Ok(NotificationExpiry::Never),
        1.. => Ok(NotificationExpiry::After(
            u32::try_from(milliseconds).unwrap_or(u32::MAX),
        )),
        _ => Err(KeyError::Unusable),
    }
}

/// What the payload of one OSC 99 is, from its `p` key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Payload {
    Title,
    Body,
    Buttons,
    Icon,
    /// `?`: which parts of the protocol are supported.
    Query,
    /// `close`: close the notification shown under this id.
    Close,
    /// `alive`: which notifications are still shown.
    Alive,
    /// A type newer than this code.
    Unknown,
}

/// The keys of one OSC 99 that are about the chunk itself.
struct Chunk {
    id: String,
    payload: Payload,
    done: bool,
    base64: bool,
}

impl Chunk {
    /// `None` when the id is longer than [`NOTIFICATION_ID_MAX`].
    fn parse(metadata: &[u8]) -> Option<Chunk> {
        let mut chunk = Chunk {
            id: String::new(),
            payload: Payload::Title,
            done: true,
            base64: false,
        };
        for (key, value) in pairs(metadata) {
            match key {
                b'i' => chunk.id = clean_id_within_cap(value).ok()?,
                b'p' => {
                    chunk.payload = match value {
                        b"title" => Payload::Title,
                        b"body" => Payload::Body,
                        b"buttons" => Payload::Buttons,
                        b"icon" => Payload::Icon,
                        b"?" => Payload::Query,
                        b"close" => Payload::Close,
                        b"alive" => Payload::Alive,
                        _ => Payload::Unknown,
                    }
                }
                b'd' => chunk.done = value != b"0",
                b'e' => chunk.base64 = value == b"1",
                _ => {}
            }
        }
        Some(chunk)
    }
}

impl Handler<'_> {
    /// OSC 99: `99 ; metadata ; payload`.
    pub(super) fn osc_desktop_notification(&mut self, params: &OscParams<'_>, term: StringTerm) {
        let metadata = params.get(1).unwrap_or_default();
        let Some(chunk) = Chunk::parse(metadata) else {
            self.unhandled();
            return;
        };
        match chunk.payload {
            Payload::Query => self.answer_notification_query(&chunk.id, term),
            // Without an id there is nothing to close: a no-op, as the protocol says.
            Payload::Close if chunk.id.is_empty() => {}
            Payload::Close => {
                let id = self.out.text_span(&chunk.id);
                self.out.push(VtEvent::NotificationClose { id });
            }
            Payload::Alive => {
                let id = self.out.text_span(&chunk.id);
                self.out.push(VtEvent::NotificationAliveQuery { id });
            }
            _ => self.notification_chunk(chunk, metadata, params),
        }
    }

    /// The answer to `p=?`, built from what the embedder declared.
    fn answer_notification_query(&mut self, id: &str, term: StringTerm) {
        let id = if id.is_empty() { "0" } else { id };
        let end = match term {
            StringTerm::Bel => "\x07",
            StringTerm::St => "\x1b\\",
        };
        let answer = self.state.config.notifications.answer();
        let reply = format!("\x1b]99;i={id}:p=?;{answer}{end}");
        self.out.push_reply(reply.as_bytes());
    }

    /// Adds one chunk to its notification and reports the notification when it is done.
    fn notification_chunk(&mut self, chunk: Chunk, metadata: &[u8], params: &OscParams<'_>) {
        let pending = &mut self.state.notifications;
        let mut notification = match pending.iter().position(|waiting| waiting.id == chunk.id) {
            Some(index) => pending.remove(index),
            None => PendingNotification::new(chunk.id),
        };
        let mut verdict = Verdict::default();
        notification.apply_keys(metadata, &mut verdict);
        let added = match chunk.payload {
            Payload::Title => add_text(&mut notification.title, chunk.base64, params),
            Payload::Body => add_text(&mut notification.body, chunk.base64, params),
            Payload::Buttons => add_text(&mut notification.buttons, chunk.base64, params),
            Payload::Icon => add_icon(&mut notification, chunk.base64, params),
            // A payload type this code does not know: nothing is taken from it.
            _ => Added::Cut,
        };
        match added {
            Added::Whole => {}
            Added::Cut => verdict.ignored = true,
            Added::NotBase64 => verdict.malformed = true,
        }
        if !chunk.done {
            let pending = &mut self.state.notifications;
            if pending.len() >= PENDING_NOTIFICATIONS_MAX {
                pending.remove(0);
            }
            pending.push(notification);
        } else if !self.push_notification(&mut notification) {
            verdict.ignored = true;
        }
        if verdict.ignored {
            self.unhandled();
        }
        if verdict.malformed {
            let stats = &mut self.state.stats;
            stats.malformed_sequences = stats.malformed_sequences.saturating_add(1);
        }
    }

    /// Reports a finished notification. A body without a title is the title, as the protocol
    /// says; `false`, reporting nothing, when there is no text at all.
    fn push_notification(&mut self, pending: &mut PendingNotification) -> bool {
        let (title, body) = (pending.title.text(), pending.body.text());
        let (title, body) = if title.is_empty() {
            (body, Cow::Borrowed(""))
        } else {
            (title, body)
        };
        if title.is_empty() {
            return false;
        }
        let out = &mut *self.out;
        let mut notification = Notification::text(out, NotificationSource::Osc99, &title, &body);
        notification.id = out.text_span(&pending.id);
        notification.app_name = out.text_span(&pending.app_name);
        notification.kinds = out.list(pending.kinds.iter().map(String::as_str));
        notification.icon_names = out.list(pending.icon_names.iter().map(String::as_str));
        notification.icon_id = out.text_span(&pending.icon_id);
        pending.icon.end_group();
        notification.icon = out.byte_span(&pending.icon.bytes);
        let labels = pending.buttons.text();
        // U+2028 LINE SEPARATOR stands between two labels.
        notification.buttons = out.list(labels.split('\u{2028}').filter(|label| !label.is_empty()));
        notification.sound = out.text_span(&pending.sound);
        notification.urgency = pending.urgency;
        notification.occasion = pending.occasion;
        notification.expiry = pending.expiry;
        notification.focus_on_activation = pending.focus;
        notification.report_activation = pending.report;
        notification.report_close = pending.report_close;
        out.push_notification(notification);
        true
    }
}

/// Adds the payload of a chunk to a text part.
fn add_text(part: &mut Part, base64: bool, params: &OscParams<'_>) -> Added {
    if base64 {
        return match (params.get(2), params.len()) {
            (None, _) => Added::Whole,
            (Some(encoded), 3) => part.add_base64(encoded, NOTIFICATION_TEXT_MAX),
            // A second piece means the payload held a `;`.
            _ => Added::NotBase64,
        };
    }
    // The parser split the payload on `;`; the text has them back.
    let mut added = Added::Whole;
    for (index, piece) in params.iter().enumerate().skip(2) {
        if index > 2 && part.add_plain(b";", NOTIFICATION_TEXT_MAX) == Added::Cut {
            added = Added::Cut;
        }
        if part.add_plain(piece, NOTIFICATION_TEXT_MAX) == Added::Cut {
            added = Added::Cut;
        }
    }
    added
}

/// Adds the payload of a chunk to the icon. Image data is binary, so only base64 is taken; an
/// icon past its cap is dropped whole, since a cut image is no image.
fn add_icon(notification: &mut PendingNotification, base64: bool, params: &OscParams<'_>) -> Added {
    if notification.icon_refused || !base64 {
        return Added::Cut;
    }
    let added = match (params.get(2), params.len()) {
        (None, _) => Added::Whole,
        (Some(encoded), 3) => notification.icon.add_base64(encoded, NOTIFICATION_ICON_MAX),
        _ => Added::NotBase64,
    };
    if added == Added::Cut {
        notification.icon = Part::default();
        notification.icon_refused = true;
    }
    added
}
