//! The event list of one `feed` call and the arena its payloads live in.

use crate::events::{ClipboardKind, VtEvent};
use crate::grid::ScrollReport;
use crate::parser::{MAX_OSC_PARAMS, OscParams, StringTerm};

/// Arena bytes kept past [`EventBatch::clear`]; a larger capacity is shrunk back to this.
pub(crate) const EVENT_ARENA_SOFT: usize = 1 << 20;
/// Event and parameter-span entries kept past [`EventBatch::clear`]; a larger capacity is
/// shrunk back to this.
pub(crate) const EVENT_LIST_SOFT: usize = 4096;

/// The events one `feed` produced, in byte order, with the bytes their payloads point into.
///
/// An event is a value in a caller-owned batch, never a callback: no embedder code runs inside
/// `feed`. Payloads are spans ([`StrSpan`], [`ByteSpan`], [`ParamSpans`]) read back through the
/// batch that produced them. A span is an index, not a borrow: kept past the next `feed` it
/// reads other bytes (still valid UTF-8 for [`EventBatch::str`]), never anything unsafe.
///
/// Every `feed` clears the batch first, and a batch that grew past 1 MiB of payload or 4096
/// events shrinks back on that clear, so one hostile feed cannot pin its peak for the session.
#[derive(Debug, Default)]
pub struct EventBatch {
    events: Vec<VtEvent>,
    arena: Vec<u8>,
    params: Vec<ByteSpan>,
    repaint: bool,
    /// Payloads dropped because the arena would have passed `u32::MAX` bytes.
    overflowed: u32,
}

/// A UTF-8 payload in an [`EventBatch`], read back with [`EventBatch::str`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StrSpan {
    start: u32,
    len: u32,
}

/// A byte payload in an [`EventBatch`], read back with [`EventBatch::bytes`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ByteSpan {
    start: u32,
    len: u32,
}

/// The parameters of a forwarded OSC, read back with [`EventBatch::params`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ParamSpans {
    first: u32,
    count: u32,
}

impl EventBatch {
    /// An empty batch.
    pub fn new() -> EventBatch {
        EventBatch::default()
    }

    /// The events in byte order.
    pub fn events(&self) -> &[VtEvent] {
        &self.events
    }

    /// The events in byte order.
    pub fn iter(&self) -> std::slice::Iter<'_, VtEvent> {
        self.events.iter()
    }

    /// The number of events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether there is no event.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The text of a span; `""` when the span lies outside this batch.
    pub fn str(&self, span: StrSpan) -> &str {
        self.slice(span.start, span.len)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .unwrap_or("")
    }

    /// The bytes of a span; empty when the span lies outside this batch.
    pub fn bytes(&self, span: ByteSpan) -> &[u8] {
        self.slice(span.start, span.len).unwrap_or_default()
    }

    /// The parameters of a forwarded OSC in order; empty when the spans lie outside this batch.
    pub fn params(&self, spans: ParamSpans) -> impl Iterator<Item = &[u8]> + '_ {
        let first = spans.first as usize;
        let end = first.saturating_add(spans.count as usize);
        self.params
            .get(first..end)
            .unwrap_or_default()
            .iter()
            .map(|span| self.bytes(*span))
    }

    /// Empties the batch, then shrinks what a large feed grew.
    ///
    /// Every `feed` calls it first; callers need not.
    pub fn clear(&mut self) {
        self.events.clear();
        self.arena.clear();
        self.params.clear();
        self.repaint = false;
        self.overflowed = 0;
        if self.arena.capacity() > EVENT_ARENA_SOFT {
            self.arena.shrink_to(EVENT_ARENA_SOFT);
        }
        if self.events.capacity() > EVENT_LIST_SOFT {
            self.events.shrink_to(EVENT_LIST_SOFT);
        }
        if self.params.capacity() > EVENT_LIST_SOFT {
            self.params.shrink_to(EVENT_LIST_SOFT);
        }
    }

    /// Appends an event. A payload-carrying event must hold spans this batch owns.
    /// [`VtEvent::Repaint`] is added at most once, like [`push_repaint`](Self::push_repaint).
    pub fn push(&mut self, event: VtEvent) {
        if event == VtEvent::Repaint {
            self.push_repaint();
        } else {
            self.events.push(event);
        }
    }

    /// Appends [`VtEvent::Repaint`] unless the batch already has one.
    pub fn push_repaint(&mut self) {
        if !self.repaint {
            self.repaint = true;
            self.events.push(VtEvent::Repaint);
        }
    }

    /// Appends [`VtEvent::Title`]. Returns `false`, pushing nothing, when `text` is not UTF-8.
    pub fn push_title(&mut self, text: &[u8]) -> bool {
        self.push_text(text, VtEvent::Title)
    }

    /// Appends [`VtEvent::ClipboardStore`]. Returns `false`, pushing nothing, when `text` is
    /// not UTF-8.
    pub fn push_clipboard_store(&mut self, selection: ClipboardKind, text: &[u8]) -> bool {
        self.push_text(text, |text| VtEvent::ClipboardStore { selection, text })
    }

    /// Appends [`VtEvent::Reply`]: bytes the embedder writes to the program as they are.
    pub fn push_reply(&mut self, bytes: &[u8]) {
        if let Some(span) = self.append(bytes) {
            self.events.push(VtEvent::Reply(ByteSpan {
                start: span.0,
                len: span.1,
            }));
        }
    }

    /// Appends [`VtEvent::Osc`] with one span per element of `params`.
    pub fn push_osc(
        &mut self,
        code: u32,
        params: &[&[u8]],
        terminator: StringTerm,
        truncated: bool,
    ) {
        let first = self.next_param();
        for param in params {
            self.push_param(param);
        }
        self.finish_osc(code, first, terminator, truncated);
    }

    /// Current capacity of the payload arena in bytes.
    pub fn arena_capacity(&self) -> usize {
        self.arena.capacity()
    }

    /// Current capacity of the event list.
    pub fn event_capacity(&self) -> usize {
        self.events.capacity()
    }

    /// Current capacity of the OSC parameter-span list.
    pub fn param_capacity(&self) -> usize {
        self.params.capacity()
    }

    /// The payload bytes at `start..start + len`, if inside the arena.
    fn slice(&self, start: u32, len: u32) -> Option<&[u8]> {
        let start = start as usize;
        self.arena.get(start..start.checked_add(len as usize)?)
    }

    /// Copies `bytes` into the arena. `None`, counting an overflow, when the arena would pass
    /// `u32::MAX` bytes; the arena never does, so every offset fits `u32`.
    fn append(&mut self, bytes: &[u8]) -> Option<(u32, u32)> {
        let span = u32::try_from(self.arena.len())
            .ok()
            .zip(u32::try_from(bytes.len()).ok())
            .filter(|(start, len)| start.checked_add(*len).is_some());
        let Some((start, len)) = span else {
            self.overflowed = self.overflowed.saturating_add(1);
            return None;
        };
        self.arena.extend_from_slice(bytes);
        Some((start, len))
    }

    /// The index the next parameter span will get.
    fn next_param(&self) -> u32 {
        u32::try_from(self.params.len()).unwrap_or(u32::MAX)
    }

    fn push_param(&mut self, bytes: &[u8]) {
        let span = self.append(bytes).unwrap_or((0, 0));
        self.params.push(ByteSpan {
            start: span.0,
            len: span.1,
        });
    }

    fn finish_osc(&mut self, code: u32, first: u32, terminator: StringTerm, truncated: bool) {
        let count = self.next_param().saturating_sub(first);
        self.events.push(VtEvent::Osc {
            code,
            params: ParamSpans { first, count },
            terminator,
            truncated,
        });
    }

    /// Appends the UTF-8 `text` as a payload and the event `make` builds from its span.
    /// Returns `false`, pushing nothing, when `text` is not UTF-8.
    pub(crate) fn push_text(&mut self, text: &[u8], make: impl FnOnce(StrSpan) -> VtEvent) -> bool {
        if std::str::from_utf8(text).is_err() {
            return false;
        }
        let Some((start, len)) = self.append(text) else {
            return false;
        };
        self.events.push(make(StrSpan { start, len }));
        true
    }

    /// Appends a forwarded OSC: every parser parameter, the last one re-split on `;` so each
    /// `;`-separated parameter is one span (at most payload bytes + 1 spans).
    pub(crate) fn push_osc_params(
        &mut self,
        code: u32,
        params: &OscParams<'_>,
        terminator: StringTerm,
        truncated: bool,
    ) {
        let first = self.next_param();
        for (index, param) in params.iter().enumerate() {
            if index + 1 == MAX_OSC_PARAMS {
                for piece in param.split(|byte| *byte == b';') {
                    self.push_param(piece);
                }
            } else {
                self.push_param(param);
            }
        }
        self.finish_osc(code, first, terminator, truncated);
    }

    /// Appends the events of a scroll report: [`VtEvent::RowsScrolled`] if rows moved, then
    /// [`VtEvent::RowsTrimmed`] if rows left the history.
    pub(crate) fn push_scroll_report(&mut self, report: &ScrollReport) {
        if let Some(scrolled) = report.scrolled {
            self.events.push(VtEvent::RowsScrolled(scrolled));
        }
        if let Some(oldest) = report.trimmed {
            self.events.push(VtEvent::RowsTrimmed { oldest });
        }
    }

    /// Payloads dropped so far because the arena would have passed `u32::MAX` bytes.
    pub(crate) fn overflowed(&self) -> u32 {
        self.overflowed
    }

    /// Where the next payload will start; pair with [`extend`](Self::extend) and one of the
    /// `finish_*` methods.
    pub(crate) fn mark(&self) -> usize {
        self.arena.len()
    }

    /// Appends payload bytes after a [`mark`](Self::mark); bytes that would pass the offset
    /// limit are dropped and counted.
    pub(crate) fn extend(&mut self, bytes: &[u8]) {
        let _ = self.append(bytes);
    }

    /// Ends a payload built since `mark`: trims Unicode whitespace at both ends. Invalid UTF-8
    /// rewinds the arena to `mark` and gives `None`.
    pub(crate) fn finish_trimmed(&mut self, mark: usize) -> Option<StrSpan> {
        let built = self.arena.get(mark..).unwrap_or_default();
        let Ok(text) = std::str::from_utf8(built) else {
            self.arena.truncate(mark);
            return None;
        };
        let trimmed = text.trim();
        let lead = text.len() - text.trim_start().len();
        Some(StrSpan {
            start: u32::try_from(mark + lead).unwrap_or(u32::MAX),
            len: u32::try_from(trimmed.len()).unwrap_or(u32::MAX),
        })
    }

    /// Ends a payload built since `mark`, replacing invalid UTF-8 with `U+FFFD`; allocates
    /// only then.
    pub(crate) fn finish_lossy(&mut self, mark: usize) -> StrSpan {
        let built = self.arena.get(mark..).unwrap_or_default();
        if let std::borrow::Cow::Owned(repaired) = String::from_utf8_lossy(built) {
            self.arena.truncate(mark);
            self.extend(repaired.as_bytes());
        }
        StrSpan {
            start: u32::try_from(mark).unwrap_or(u32::MAX),
            len: u32::try_from(self.arena.len().saturating_sub(mark)).unwrap_or(u32::MAX),
        }
    }

    /// Stores `text` as a payload.
    pub(crate) fn text_span(&mut self, text: &str) -> StrSpan {
        let mark = self.mark();
        self.extend(text.as_bytes());
        self.finish_lossy(mark)
    }

    /// Decodes canonical base64 into the arena: a length that is a multiple of 4, the alphabet
    /// `A-Z a-z 0-9 + /`, `=` only as the last one or two characters, and UTF-8 output. Anything
    /// else rewinds the arena and gives `None`.
    pub(crate) fn decode_base64(&mut self, encoded: &[u8]) -> Option<StrSpan> {
        let mark = self.mark();
        if !encoded.len().is_multiple_of(4) {
            return None;
        }
        let quanta = encoded.len() / 4;
        for (index, quantum) in encoded.chunks_exact(4).enumerate() {
            let last = index + 1 == quanta;
            if !self.decode_quantum(quantum, last) {
                self.arena.truncate(mark);
                return None;
            }
        }
        let built = self.arena.get(mark..).unwrap_or_default();
        if std::str::from_utf8(built).is_err() {
            self.arena.truncate(mark);
            return None;
        }
        Some(StrSpan {
            start: u32::try_from(mark).unwrap_or(u32::MAX),
            len: u32::try_from(built.len()).unwrap_or(u32::MAX),
        })
    }

    /// Decodes four characters; `false` when they are not canonical.
    fn decode_quantum(&mut self, quantum: &[u8], last: bool) -> bool {
        let padding = quantum
            .iter()
            .rev()
            .take_while(|byte| **byte == b'=')
            .count();
        if padding > 2 || (padding > 0 && !last) {
            return false;
        }
        let mut bits = 0u32;
        for byte in quantum.iter().take(4 - padding) {
            let Some(value) = base64_value(*byte) else {
                return false;
            };
            bits = (bits << 6) | u32::from(value);
        }
        bits <<= 6 * padding as u32;
        let bytes = bits.to_be_bytes();
        self.extend(bytes.get(1..4 - padding).unwrap_or_default());
        true
    }
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}
