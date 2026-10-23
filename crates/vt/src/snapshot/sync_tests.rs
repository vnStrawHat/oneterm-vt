//! Tests of synchronized output with an injected clock: every instant is the session's base
//! instant plus an offset, so nothing here sleeps or reads the real time.

use std::time::{Duration, Instant};

use super::super::snapshot_tests::Session;
use super::super::{SnapshotUpdate, SyncState};
use super::{SYNC_REFRESH, SYNC_WATCHDOG};

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// A session whose first update has been taken.
fn settled() -> Session {
    let mut session = Session::new();
    session.update();
    session
}

#[test]
fn block_suppresses_until_close() {
    let mut s = settled();
    let t = s.now;
    s.feed_at(b"\x1b[?2026h\x1b[3;1Hx", t);
    assert_eq!(s.update_at(t + SYNC_REFRESH / 2), SnapshotUpdate::Unchanged);
    assert!(s.state.changed().is_empty());
    s.feed_at(b"\x1b[?2026l", t + SYNC_REFRESH / 2);
    assert_eq!(
        s.update_at(t + SYNC_REFRESH / 2),
        SnapshotUpdate::Partial { scrolled: 0 }
    );
    assert_eq!(s.state.changed(), [2]);
}

#[test]
fn refresh_deadline_resumes_frames() {
    let mut s = settled();
    let t = s.now;
    s.feed_at(b"\x1b[?2026h\x1b[2;1Hx", t);
    assert_eq!(
        s.update_at(t + SYNC_REFRESH - ms(1)),
        SnapshotUpdate::Unchanged
    );
    assert_eq!(
        s.update_at(t + SYNC_REFRESH),
        SnapshotUpdate::Partial { scrolled: 0 }
    );
    assert_eq!(s.state.changed(), [1]);
    assert!(s.term.sync().is_set());
}

#[test]
fn watchdog_forces_the_mode_off() {
    let mut s = settled();
    let t = s.now;
    s.feed_at(b"\x1b[?2026h\x1b[4;1Hx", t);
    // A program that keeps refreshing the block holds frames until the watchdog.
    let half = SYNC_REFRESH / 2;
    let mut at = t + half;
    while at < t + SYNC_WATCHDOG {
        s.feed_at(b"\x1b[?2026h", at);
        assert_eq!(s.update_at(at), SnapshotUpdate::Unchanged, "{:?}", at - t);
        at += half;
    }
    assert_eq!(
        s.update_at(t + SYNC_WATCHDOG),
        SnapshotUpdate::Partial { scrolled: 0 }
    );
    assert_eq!(s.state.changed(), [3]);
    assert!(!s.term.sync().is_set());
    s.feed_at(b"\x1b[?2026$p", t + SYNC_WATCHDOG);
    assert_eq!(s.replies(), "\x1b[?2026;2$y");
}

#[test]
fn decrqm_reports_the_real_state() {
    let mut s = Session::new();
    s.feed(b"\x1b[?2026$p");
    assert_eq!(s.replies(), "\x1b[?2026;2$y");
    s.feed(b"\x1b[?2026h\x1b[?2026$p");
    assert_eq!(s.replies(), "\x1b[?2026;1$y");
    s.feed(b"\x1b[?2026l\x1b[?2026$p");
    assert_eq!(s.replies(), "\x1b[?2026;2$y");
}

#[test]
fn first_update_in_a_block_builds() {
    let mut s = Session::new();
    s.feed(b"\x1b[?2026h");
    assert_eq!(s.update(), SnapshotUpdate::Full);
    assert_eq!(s.state.rows().len(), 10);
    assert_eq!(s.update(), SnapshotUpdate::Unchanged);
}

#[test]
fn mode_change_in_a_block_reaches_the_next_frame() {
    let mut s = settled();
    let t = s.now;
    s.feed_at(b"\x1b[?2026h\x1b[?25l", t);
    assert_eq!(s.update_at(t + ms(10)), SnapshotUpdate::Unchanged);
    s.feed_at(b"\x1b[?2026l", t + ms(10));
    assert_eq!(
        s.update_at(t + ms(10)),
        SnapshotUpdate::Partial { scrolled: 0 }
    );
    assert!(s.state.changed().is_empty());
    assert_eq!(s.update_at(t + ms(10)), SnapshotUpdate::Unchanged);
}

#[test]
fn cursor_move_in_a_block_reaches_the_next_frame() {
    let mut s = settled();
    let t = s.now;
    s.feed_at(b"\x1b[?2026h\x1b[5;10H", t);
    assert_eq!(s.update_at(t + ms(10)), SnapshotUpdate::Unchanged);
    assert_eq!((s.state.cursor().row, s.state.cursor().col), (Some(4), 9));
    s.feed_at(b"\x1b[?2026l", t + ms(10));
    assert_eq!(
        s.update_at(t + ms(10)),
        SnapshotUpdate::Partial { scrolled: 0 }
    );
    assert_eq!((s.state.cursor().row, s.state.cursor().col), (Some(4), 9));
    assert_eq!(s.update_at(t + ms(10)), SnapshotUpdate::Unchanged);
}

#[test]
fn modes_refresh_while_suppressed() {
    let mut s = settled();
    let t = s.now;
    s.feed_at(b"\x1b[?2026h\x1b[?2004h\x1b[?1h", t);
    assert_eq!(s.update_at(t + ms(10)), SnapshotUpdate::Unchanged);
    assert!(s.state.modes().bracketed_paste);
    assert!(s.state.modes().app_cursor);
}

#[test]
fn ris_ends_the_block() {
    let mut s = settled();
    s.feed(b"\x1b[?2026h");
    assert!(s.term.sync().is_set());
    s.feed(b"\x1bc");
    assert!(!s.term.sync().is_set());
}

#[test]
fn decstr_keeps_it() {
    let mut s = settled();
    s.feed(b"\x1b[?2026h");
    s.feed(b"\x1b[!p");
    assert!(s.term.sync().is_set());
}

#[test]
fn deadline_reports_the_next_wake() {
    let t = Instant::now();
    let mut sync = SyncState::new();
    assert_eq!(sync.deadline(), None);
    assert!(!sync.is_set());
    sync.begin(t);
    assert_eq!(sync.deadline(), Some(t + SYNC_REFRESH));
    sync.begin(t + ms(900));
    assert_eq!(sync.deadline(), Some(t + SYNC_WATCHDOG));
    sync.end();
    assert_eq!(sync.deadline(), None);
}

#[test]
fn closed_state_never_suppresses() {
    let t = Instant::now();
    let mut sync = SyncState::new();
    assert!(!sync.suppresses_frame(t));
    sync.begin(t);
    assert!(sync.suppresses_frame(t));
    sync.end();
    assert!(!sync.suppresses_frame(t));
}

#[test]
fn absurd_clock_does_not_panic() {
    // Walk to the largest instant the platform represents.
    let mut far = Instant::now();
    let mut step = Duration::from_secs(1 << 62);
    while !step.is_zero() {
        match far.checked_add(step) {
            Some(next) => far = next,
            None => step /= 2,
        }
    }
    let mut sync = SyncState::new();
    sync.begin(far);
    assert!(!sync.suppresses_frame(far));
    assert!(!sync.is_set());
    sync.begin(far);
    assert!(sync.deadline().is_some());
}
