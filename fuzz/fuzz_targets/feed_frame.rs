//! Fuzzes `Terminal::feed_frame` against `Terminal::feed`: one stream through both must give the
//! same terminal, and `feed_frame` must return exactly where a synchronized update closes.
//!
//! The input is an op stream, so that the sequences that matter turn up often. The first 4 bytes
//! are chunk lengths. After them a tag byte `t` with `t & 3 == 0` appends
//! `PIECES[(t >> 2) % PIECES.len()]`; any other tag appends the next `t >> 2` bytes as they are.
//! The oracle is a third terminal fed one byte at a time: the byte whose feed reports a closed
//! frame is where `feed_frame` has to stop. Each chunk is fed 50 ms after the previous one.
//! Run: `cargo +nightly-2026-05-25 fuzz run --target-dir target/fuzz-linux feed_frame`

#![no_main]

use std::time::{Duration, Instant};

use libfuzzer_sys::fuzz_target;
use oneterm_vt::{Config, EventBatch, ModeSnapshot, RowId, Size, Terminal};

/// Whole and broken ways to set and reset `?2026`, and the bytes that break a sequence.
const PIECES: [&[u8]; 28] = [
    b"\x1b[?2026h",
    b"\x1b[?2026l",
    b"\x1b[?2026l\x1b[?2026h",
    b"\x1b[?2026r",
    b"\x1b[?2026s",
    b"\x1b[?25;2026l",
    b"\x1b[?2026;25l",
    b"\x1b[?20\r26l",
    b"\x1b[?2\x7f026l",
    b"\x1b[?202\x806l",
    b"\x1b[",
    b"\x1b[?",
    b"\x1b[?2026",
    b"2026",
    b"20",
    b"26",
    b"l",
    b"h",
    b"r",
    b"?",
    b";",
    b"\x1b",
    b"\x18",
    b"\r",
    b"\x7f",
    b"\x80",
    b"\x1bc",
    b"frame 2026 l",
];

/// The stream the ops describe.
fn stream(ops: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = ops;
    while let Some((&tag, tail)) = rest.split_first() {
        if tag & 3 == 0 {
            out.extend_from_slice(PIECES[usize::from(tag >> 2) % PIECES.len()]);
            rest = tail;
        } else {
            let (raw, tail) = tail.split_at(usize::from(tag >> 2).min(tail.len()));
            out.extend_from_slice(raw);
            rest = tail;
        }
    }
    out
}

/// Small on purpose: the oracle feeds one byte at a time, and `vt-paranoid` walks the whole
/// history after every feed.
fn terminal() -> Terminal {
    let config = Config {
        scrollback_limit: 8,
        ..Config::default()
    };
    Terminal::new(Size { rows: 4, cols: 16 }, config)
}

/// What both ways of feeding must agree on.
fn state(term: &Terminal) -> (Vec<String>, String, Option<String>, u64, ModeSnapshot, bool) {
    let screen = term.screen();
    let rows = (screen.oldest().0..=screen.newest().0)
        .map(|id| term.row_text(RowId(id)))
        .collect();
    let cursor = format!("{:?} {}", screen.cursor().pos, screen.cursor().pending_wrap);
    (
        rows,
        cursor,
        term.title().map(str::to_owned),
        term.lines_produced(),
        term.mode_snapshot(),
        term.sync().is_set(),
    )
}

fuzz_target!(|data: &[u8]| {
    let (lengths, ops) = data.split_at(data.len().min(4));
    if lengths.is_empty() {
        return;
    }
    let stream = stream(ops);
    let start = Instant::now();
    let mut batch = EventBatch::new();

    let mut oracle = terminal();
    let mut closes = Vec::new();
    for (index, byte) in stream.iter().enumerate() {
        let stats = oracle.feed(std::slice::from_ref(byte), &mut batch, start);
        if stats.frames_closed > 0 {
            closes.push(index + 1);
        }
    }

    let mut plain = terminal();
    let mut framed = terminal();
    let mut plain_closed = 0;
    let mut stops = Vec::new();
    let mut offset = 0;
    for (index, &length) in lengths.iter().cycle().enumerate() {
        if offset >= stream.len() {
            break;
        }
        let end = (offset + 1 + usize::from(length) % 64).min(stream.len());
        let now = start + Duration::from_millis(50 * index as u64);
        let chunk = &stream[offset..end];
        plain_closed += plain.feed(chunk, &mut batch, now).frames_closed as usize;
        let mut rest = chunk;
        while !rest.is_empty() {
            let stats = framed.feed_frame(rest, &mut batch, now);
            assert!((1..=rest.len()).contains(&stats.bytes), "no progress");
            rest = &rest[stats.bytes..];
            if stats.frames_closed > 0 {
                assert_eq!(stats.frames_closed, 1, "passed a closed frame");
                assert!(
                    !framed.sync().is_set(),
                    "stopped after the next block opened"
                );
                stops.push(end - rest.len());
            }
        }
        offset = end;
    }

    assert_eq!(
        stops, closes,
        "feed_frame did not stop where a frame closed"
    );
    assert_eq!(plain_closed, closes.len());
    assert_eq!(state(&framed), state(&plain));
    assert_eq!(state(&plain), state(&oracle));
});
