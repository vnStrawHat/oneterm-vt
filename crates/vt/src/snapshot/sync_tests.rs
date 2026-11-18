//! Tests of synchronized output with an injected clock: every instant is the session's base
//! instant plus an offset, so nothing here sleeps or reads the real time.

use std::time::{Duration, Instant};

use super::super::snapshot_tests::Session;
use super::super::{SnapshotContent, SnapshotRow, SnapshotUpdate, SyncState};
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

// ---- frames closed, and feeding up to one ----

/// The text of every viewport row as the state holds it.
fn screen_text(session: &Session) -> Vec<String> {
    let text = |row: &SnapshotRow| {
        let text: String = row
            .cells
            .iter()
            .map(|cell| match cell.content {
                SnapshotContent::Scalar(c) => c,
                SnapshotContent::Cluster { .. } => '?',
            })
            .collect();
        text.trim_end().to_owned()
    };
    session.state.rows().iter().map(text).collect()
}

/// The text of viewport row 0 as the state holds it.
fn top_row(session: &Session) -> String {
    screen_text(session).swap_remove(0)
}

const TWO_FRAMES: &[u8] = b"\x1b[?2026h\x1b[HONE\x1b[?2026l\x1b[?2026h\x1b[HTWO\x1b[?2026l";
/// The length of the first frame of [`TWO_FRAMES`], its closing sequence included.
const FIRST_FRAME: usize = 22;

#[test]
fn feed_counts_the_frames_it_closed() {
    let mut s = settled();
    let stats = s.term.feed(TWO_FRAMES, &mut s.batch, s.now);
    assert_eq!((stats.bytes, stats.frames_closed), (TWO_FRAMES.len(), 2));
    // A reset that finds the mode off closes nothing.
    let stats = s.term.feed(b"\x1b[?2026l", &mut s.batch, s.now);
    assert_eq!(stats.frames_closed, 0);
}

#[test]
fn back_to_back_blocks_hide_the_closed_frame_from_feed() {
    let mut s = settled();
    // One read: frame 1 closed, frame 2 opened. The frame in between cannot be sampled.
    s.feed(&TWO_FRAMES[..FIRST_FRAME + 12]);
    assert_eq!(s.update(), SnapshotUpdate::Unchanged);
    assert_eq!(top_row(&s), "");
}

#[test]
fn feed_frame_returns_after_each_closed_frame() {
    let mut s = settled();
    let mut rest = TWO_FRAMES;
    let mut frames = Vec::new();
    while !rest.is_empty() {
        let stats = s.term.feed_frame(rest, &mut s.batch, s.now);
        rest = &rest[stats.bytes..];
        if stats.frames_closed > 0 {
            assert_eq!(stats.frames_closed, 1);
            assert!(!s.term.sync().is_set());
            assert_ne!(s.update(), SnapshotUpdate::Unchanged, "not held here");
            frames.push(top_row(&s));
        }
    }
    assert_eq!(frames, ["ONE", "TWO"]);
}

#[test]
fn feed_frame_reports_the_bytes_it_applied() {
    let mut s = settled();
    let stats = s.term.feed_frame(TWO_FRAMES, &mut s.batch, s.now);
    assert_eq!((stats.bytes, stats.frames_closed), (FIRST_FRAME, 1));
    assert_eq!(s.term.stats().bytes, FIRST_FRAME);
    // With no closed frame it is `feed`.
    let open = b"plain \x1b[?2026l text";
    let stats = s.term.feed_frame(open, &mut s.batch, s.now);
    assert_eq!((stats.bytes, stats.frames_closed), (open.len(), 0));
    assert_eq!(s.term.feed_frame(b"", &mut s.batch, s.now).bytes, 0);
}

#[test]
fn feed_frame_stops_at_a_close_split_across_reads() {
    for cut in 1..TWO_FRAMES.len() {
        let mut s = settled();
        let (head, tail) = TWO_FRAMES.split_at(cut);
        let mut closed = Vec::new();
        for mut rest in [head, tail] {
            while !rest.is_empty() {
                let stats = s.term.feed_frame(rest, &mut s.batch, s.now);
                rest = &rest[stats.bytes..];
                if stats.frames_closed > 0 {
                    s.update();
                    closed.push(top_row(&s));
                }
            }
        }
        assert_eq!(closed, ["ONE", "TWO"], "cut at {cut}");
    }
}

#[test]
fn a_mode_list_that_resets_2026_is_applied_whole_before_the_stop() {
    let mut s = settled();
    let stream = b"\x1b[?2026h\x1b[?2026;25lrest";
    let stats = s.term.feed_frame(stream, &mut s.batch, s.now);
    assert_eq!((stats.bytes, stats.frames_closed), (stream.len() - 4, 1));
    assert!(!s.term.mode_snapshot().show_cursor, "?25 was reset too");
}

#[test]
fn feed_frame_reads_past_text_that_holds_the_digits() {
    let mut s = settled();
    let text = b"\x1b[?2026h2026-11-18 a log line\r\nthe year 2026\r\n\x1b[2026;1Hx";
    let stats = s.term.feed_frame(text, &mut s.batch, s.now);
    assert_eq!((stats.bytes, stats.frames_closed), (text.len(), 0));
    assert!(s.term.sync().is_set());
}

#[test]
fn feed_frame_stops_at_a_reset_whose_digits_are_not_side_by_side() {
    // A control character is executed inside a sequence, `DEL` and 8-bit bytes are dropped
    // there: each of these still resets `?2026`.
    let resets: [&[u8]; 6] = [
        b"\x1b[?20\r26l",
        b"\x1b[?2\x7f026l",
        b"\x1b[?202\x806l",
        b"\x1b[\n?2026l",
        b"\x1b[?2\r0\x7f2\x806l",
        b"\x1b[?25;20\t26l",
    ];
    for reset in resets {
        let stream = [b"\x1b[?2026h\x1b[HONE", reset, b"\x1b[?2026h\x1b[HTWO"].concat();
        let close = stream.len() - 14;
        for cut in 0..=stream.len() {
            let mut s = settled();
            let (head, tail) = stream.split_at(cut);
            let mut fed = 0;
            let mut stopped_at = None;
            for mut rest in [head, tail] {
                while !rest.is_empty() {
                    let stats = s.term.feed_frame(rest, &mut s.batch, s.now);
                    rest = &rest[stats.bytes..];
                    fed += stats.bytes;
                    if stats.frames_closed > 0 {
                        assert_eq!(stats.frames_closed, 1);
                        s.update();
                        assert_eq!(top_row(&s), "ONE", "{reset:?} cut at {cut}");
                        stopped_at = Some(fed);
                    }
                }
            }
            assert_eq!(stopped_at, Some(close), "{reset:?} cut at {cut}");
        }
    }
}

#[test]
fn feed_frame_gives_the_screen_feed_gives() {
    let stream = [
        TWO_FRAMES,
        b"\r\ntail \x1b[?2026h\x1b[3;1Hthree\x1b[?2026l end",
    ]
    .concat();
    let mut whole = settled();
    whole.feed(&stream);
    whole.update();
    for cut in 0..stream.len() {
        let mut s = settled();
        let (head, tail) = stream.split_at(cut);
        let mut closed = 0;
        for mut rest in [head, tail] {
            while !rest.is_empty() {
                let stats = s.term.feed_frame(rest, &mut s.batch, s.now);
                rest = &rest[stats.bytes..];
                closed += stats.frames_closed;
            }
        }
        s.update();
        assert_eq!(closed, 3, "cut at {cut}");
        assert_eq!(screen_text(&s), screen_text(&whole), "cut at {cut}");
    }
}
