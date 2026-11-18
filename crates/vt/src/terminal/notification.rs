//! Desktop notifications sent with OSC 99: text that arrives in chunks, plain or base64, and
//! the query a program uses to find out that the protocol is understood.
//!
//! The engine gathers the title and the body and reports them as one
//! [`VtEvent::Notification`]; showing it is the embedder's. Everything else the protocol can
//! carry (icons, buttons, sounds, urgency, reports back to the program) has no event, and the
//! answer to the query says so.

use std::borrow::Cow;

use crate::events::{VtEvent, base64_value};
use crate::parser::{OscParams, StringTerm};

use super::dispatch::Handler;

/// Notifications that may wait for their last chunk at once; one more drops the one that
/// waited longest.
pub(super) const PENDING_NOTIFICATIONS_MAX: usize = 8;
/// Bytes kept of a title, and of a body; what a chunk adds past it is dropped.
pub(super) const NOTIFICATION_TEXT_MAX: usize = 4096;
/// Longest notification id, counted after the characters an id may not hold are removed.
pub(super) const NOTIFICATION_ID_MAX: usize = 128;

/// A notification whose last chunk (`d=1`, or no `d`) has not arrived.
#[derive(Debug)]
pub(super) struct PendingNotification {
    id: String,
    title: TextPart,
    body: TextPart,
}

/// One text of a notification, gathered chunk by chunk. Bytes, not a string: a chunk may end
/// inside a character.
#[derive(Debug, Default)]
struct TextPart {
    bytes: Vec<u8>,
    /// The bits of the base64 group the last chunk ended inside.
    group: u32,
    /// How many characters gave those bits, `0..=3`.
    group_len: u8,
}

/// What a chunk did to a [`TextPart`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Added {
    /// All of it was kept.
    Whole,
    /// The text is at [`NOTIFICATION_TEXT_MAX`]; the rest was dropped.
    Cut,
    /// It was not base64; nothing was kept.
    NotBase64,
}

impl TextPart {
    fn add_plain(&mut self, bytes: &[u8]) -> Added {
        self.bytes.extend_from_slice(bytes);
        self.enforce_cap()
    }

    /// Adds a piece of a base64 stream. The stream may be cut anywhere, also inside a group
    /// of four characters, and `=` ends a group early, so both chunks encoded one by one and
    /// one encoding cut into chunks decode.
    fn add_base64(&mut self, encoded: &[u8]) -> Added {
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
            if self.bytes.len() > NOTIFICATION_TEXT_MAX {
                // Nothing past the cap is kept, so the rest need not be read.
                break;
            }
        }
        self.enforce_cap()
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

    fn enforce_cap(&mut self) -> Added {
        if self.bytes.len() <= NOTIFICATION_TEXT_MAX {
            return Added::Whole;
        }
        self.bytes.truncate(NOTIFICATION_TEXT_MAX);
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

/// What the payload of one OSC 99 is, from its `p` key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Payload {
    Title,
    Body,
    /// `?`: which parts of the protocol are understood.
    Query,
    /// `close` and `alive`: about a notification already shown, which the engine never sees.
    AboutShown,
    /// `icon`, `buttons`, or a type newer than this code: part of a notification, but
    /// nothing the event carries.
    NotCarried,
}

/// The metadata of one OSC 99: `key=value` pairs separated by `:`. A key the engine has no
/// use for is skipped, as the protocol asks.
struct Metadata {
    id: String,
    payload: Payload,
    done: bool,
    base64: bool,
}

impl Metadata {
    /// `None` when the id is longer than [`NOTIFICATION_ID_MAX`].
    fn parse(metadata: &[u8]) -> Option<Metadata> {
        let mut parsed = Metadata {
            id: String::new(),
            payload: Payload::Title,
            done: true,
            base64: false,
        };
        for pair in metadata.split(|byte| *byte == b':') {
            let [key, b'=', value @ ..] = pair else {
                continue;
            };
            match *key {
                b'i' => parsed.id = sanitized_id(value)?,
                b'p' => {
                    parsed.payload = match value {
                        b"title" => Payload::Title,
                        b"body" => Payload::Body,
                        b"?" => Payload::Query,
                        b"close" | b"alive" => Payload::AboutShown,
                        _ => Payload::NotCarried,
                    }
                }
                b'd' => parsed.done = value != b"0",
                b'e' => parsed.base64 = value == b"1",
                _ => {}
            }
        }
        Some(parsed)
    }
}

/// The id with every character outside `a-z A-Z 0-9 _ - + .` removed: an id is echoed in
/// replies, so nothing else may get into one. `None` when what is left is too long.
fn sanitized_id(id: &[u8]) -> Option<String> {
    let clean: String = id
        .iter()
        .filter(|byte| byte.is_ascii_alphanumeric() || b"_-+.".contains(byte))
        .map(|byte| char::from(*byte))
        .collect();
    (clean.len() <= NOTIFICATION_ID_MAX).then_some(clean)
}

impl Handler<'_> {
    /// OSC 99: `99 ; metadata ; payload`.
    pub(super) fn osc_desktop_notification(&mut self, params: &OscParams<'_>, term: StringTerm) {
        let Some(meta) = Metadata::parse(params.get(1).unwrap_or_default()) else {
            self.unhandled();
            return;
        };
        match meta.payload {
            Payload::Query => self.answer_notification_query(&meta.id, term),
            Payload::AboutShown => self.unhandled(),
            _ => self.notification_chunk(meta, params),
        }
    }

    /// The answer to `p=?`: the payload types and the one occasion the engine understands.
    /// Actions, close reports, urgency, expiry and sounds are left out, which says they are
    /// not supported.
    fn answer_notification_query(&mut self, id: &str, term: StringTerm) {
        let id = if id.is_empty() { "0" } else { id };
        let end = match term {
            StringTerm::Bel => "\x07",
            StringTerm::St => "\x1b\\",
        };
        let reply = format!("\x1b]99;i={id}:p=?;o=always:p=title,body,?{end}");
        self.out.push_reply(reply.as_bytes());
    }

    /// Adds one chunk to its notification and reports the notification when it is done.
    fn notification_chunk(&mut self, meta: Metadata, params: &OscParams<'_>) {
        let pending = &mut self.state.notifications;
        let mut notification = match pending.iter().position(|waiting| waiting.id == meta.id) {
            Some(index) => pending.remove(index),
            None => PendingNotification {
                id: meta.id,
                title: TextPart::default(),
                body: TextPart::default(),
            },
        };
        let part = match meta.payload {
            Payload::Title => Some(&mut notification.title),
            Payload::Body => Some(&mut notification.body),
            _ => None,
        };
        // Something in this sequence was not acted on.
        let mut ignored = part.is_none();
        if let Some(part) = part {
            let added = if meta.base64 {
                match (params.get(2), params.len()) {
                    (None, _) => Added::Whole,
                    (Some(encoded), 3) => part.add_base64(encoded),
                    // A second piece means the payload held a `;`.
                    _ => Added::NotBase64,
                }
            } else {
                // The parser split the payload on `;`; the text has them back.
                let mut added = Added::Whole;
                for (index, piece) in params.iter().enumerate().skip(2) {
                    if index > 2 {
                        part.add_plain(b";");
                    }
                    added = part.add_plain(piece);
                }
                added
            };
            match added {
                Added::Whole => {}
                Added::Cut => ignored = true,
                Added::NotBase64 => {
                    let stats = &mut self.state.stats;
                    stats.malformed_sequences = stats.malformed_sequences.saturating_add(1);
                }
            }
        }
        if !meta.done {
            let pending = &mut self.state.notifications;
            if pending.len() >= PENDING_NOTIFICATIONS_MAX {
                pending.remove(0);
            }
            pending.push(notification);
        } else if !self.push_notification(&mut notification) {
            ignored = true;
        }
        if ignored {
            self.unhandled();
        }
    }

    /// Reports a finished notification. A body without a title is the title, as the protocol
    /// says; `false`, reporting nothing, when there is no text at all.
    fn push_notification(&mut self, notification: &mut PendingNotification) -> bool {
        let (title, body) = (notification.title.text(), notification.body.text());
        let (title, body) = if title.is_empty() {
            (body, Cow::Borrowed(""))
        } else {
            (title, body)
        };
        if title.is_empty() {
            return false;
        }
        let title = self.out.text_span(&title);
        let body = self.out.text_span(&body);
        self.out.push(VtEvent::Notification { title, body });
        true
    }
}
