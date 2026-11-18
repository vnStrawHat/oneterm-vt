//! Synchronized output (`?2026`): frames are skipped while a program redraws, bytes never are.

use std::time::{Duration, Instant};

/// How long one `CSI ? 2026 h` holds frames; a repeated `h` holds them again.
pub(crate) const SYNC_REFRESH: Duration = Duration::from_millis(150);
/// How long after the first `h` the mode is forced off, whatever else arrives.
pub(crate) const SYNC_WATCHDOG: Duration = Duration::from_secs(1);

/// The state of synchronized output (`CSI ? 2026 h` / `l`).
///
/// Bytes are applied as they arrive; only frames are skipped. Nothing is buffered, so a
/// program that opens an update and never closes it costs frames, never memory. The engine
/// reads no clock: both deadlines are checked against the `now` passed to
/// [`Terminal::snapshot_update`](crate::Terminal::snapshot_update).
///
/// While the mode is set, an update with a built [`SnapshotState`](crate::SnapshotState)
/// returns [`SnapshotUpdate::Unchanged`](crate::SnapshotUpdate::Unchanged) for 150 ms after
/// the latest `h`, and the mode is forced off one second after the first. When an update
/// returns `Unchanged` while [`is_set`](Self::is_set), ask again at
/// [`deadline`](Self::deadline) even if no output arrives.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SyncState {
    /// Frames are held until this instant; `Some` exactly while the mode is set.
    open_until: Option<Instant>,
    /// The mode is forced off at this instant; set by the first `h` of a block.
    watchdog: Option<Instant>,
}

impl SyncState {
    /// A closed state.
    pub fn new() -> SyncState {
        SyncState::default()
    }

    /// `CSI ? 2026 h`: holds frames for one refresh period. A repeated `h` moves that
    /// deadline, never the watchdog.
    pub fn begin(&mut self, now: Instant) {
        // An absurd clock value cannot be advanced; `now` itself suppresses nothing.
        let after = |wait: Duration| now.checked_add(wait).unwrap_or(now);
        self.open_until = Some(after(SYNC_REFRESH));
        self.watchdog.get_or_insert_with(|| after(SYNC_WATCHDOG));
    }

    /// `CSI ? 2026 l`, or the watchdog.
    pub fn end(&mut self) {
        *self = SyncState::default();
    }

    /// Whether the mode is set: what DECRQM reports. Stays `true` after the refresh deadline
    /// passes, until a reset or the watchdog.
    pub fn is_set(&self) -> bool {
        self.open_until.is_some()
    }

    /// Whether a frame at `now` must be skipped. Forces the mode off once the watchdog has
    /// passed.
    pub fn suppresses_frame(&mut self, now: Instant) -> bool {
        let (Some(open_until), Some(watchdog)) = (self.open_until, self.watchdog) else {
            return false;
        };
        if now >= watchdog {
            self.end();
            return false;
        }
        now < open_until
    }

    /// Whether an image release first held back at `since` must still wait at `now`: frames
    /// are held, so a held frame may show the image, and the release has waited less than the
    /// watchdog period. The bound holds whatever the program does: one that opens a block
    /// right after every close would otherwise hold releases, and their textures, for ever.
    pub(crate) fn defers_release(&self, since: Instant, now: Instant) -> bool {
        let held = matches!(
            (self.open_until, self.watchdog),
            (Some(open_until), Some(watchdog)) if now < open_until && now < watchdog
        );
        held && now.saturating_duration_since(since) < SYNC_WATCHDOG
    }

    /// The instant a held-back renderer must ask again: the earlier of the refresh deadline
    /// and the watchdog. `None` while the mode is off.
    pub fn deadline(&self) -> Option<Instant> {
        Some(self.open_until?.min(self.watchdog?))
    }
}

/// The start of `bytes` up to and including the final byte of the first sequence that may
/// reset `?2026`; all of `bytes` when there is none.
///
/// Such a sequence is `CSI ? ... l` (or `r`, a restore) and its `?` is always in the bytes,
/// so the search is for that byte. From it, the first byte in the final range ends the
/// sequence, whatever sits between: a header holds no such byte. The cut is made when the
/// final is `l` or `r` and the header has the digits `2026` or a byte that is not part of
/// it (a control character, `DEL` or an 8-bit byte may sit between the digits). With
/// `in_csi` the bytes start inside a sequence whose `?` came earlier: the cut is at its
/// final byte. A cut in the wrong place is harmless: a stream may be cut anywhere.
pub(crate) fn frame_piece(bytes: &[u8], in_csi: bool) -> &[u8] {
    let mut header = if in_csi {
        Some(0)
    } else {
        memchr::memmem::find(bytes, b"?").map(|at| at + 1)
    };
    let mut cut_any = in_csi;
    while let Some(from) = header {
        let tail = bytes.get(from..).unwrap_or_default();
        let Some(len) = tail.iter().position(|byte| (0x40..=0x7E).contains(byte)) else {
            break;
        };
        let end = from + len + 1;
        let params = tail.get(..len).unwrap_or_default();
        let resets = matches!(tail.get(len), Some(b'l' | b'r'))
            && (params.windows(4).any(|digits| digits == b"2026")
                || params.iter().any(|byte| !(0x20..=0x7E).contains(byte)));
        if cut_any || resets {
            return bytes.get(..end).unwrap_or(bytes);
        }
        cut_any = false;
        let rest = bytes.get(end..).unwrap_or_default();
        header = memchr::memmem::find(rest, b"?").map(|at| end + at + 1);
    }
    bytes
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod sync_tests;
