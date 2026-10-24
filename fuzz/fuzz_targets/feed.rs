//! Fuzzes `Terminal::feed` end to end: parser, grid, events, snapshot and graphics.
//!
//! The first 8 bytes choose the chunk lengths for the rest, and each chunk is fed 50 ms after the
//! previous one, so the synchronized-output timeout path runs. Every span of every event is read
//! back, and one snapshot update follows the last chunk.
//! Run: `cargo +nightly-2026-05-25 fuzz run --target-dir target/fuzz-linux feed`

#![no_main]

use std::hint::black_box;
use std::time::{Duration, Instant};

use libfuzzer_sys::fuzz_target;
use oneterm_vt::{Config, EventBatch, OscRoute, OscRoutes, Size, SnapshotState, Terminal, VtEvent};

/// Resolves every span of every event, so an index outside its batch panics here.
fn read_spans(batch: &EventBatch) {
    for event in batch.iter() {
        match event {
            VtEvent::Title(span) | VtEvent::IconName(span) | VtEvent::Pointer(span) => {
                black_box(batch.str(*span));
            }
            VtEvent::ClipboardStore { text, .. } => {
                black_box(batch.str(*text));
            }
            VtEvent::Cwd { host, path } => {
                black_box((batch.str(*host), batch.str(*path)));
            }
            VtEvent::Notification { title, body } => {
                black_box((batch.str(*title), batch.str(*body)));
            }
            VtEvent::Reply(span) => {
                black_box(batch.bytes(*span));
            }
            VtEvent::Osc { params, .. } => {
                for param in batch.params(*params) {
                    black_box(param);
                }
            }
            other => {
                black_box(other);
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some(&flags) = data.first() else {
        return;
    };
    let (lengths, body) = data.split_at(data.len().min(8));

    let mut osc_routes = OscRoutes::new();
    osc_routes.route(7501, OscRoute::Forward);
    osc_routes.route(52, OscRoute::BuiltinAndForward);
    let config = Config {
        scrollback_limit: 200,
        allow_screen_readback: flags & 1 == 1,
        osc_routes,
        ..Config::default()
    };
    let mut term = Terminal::new(Size { rows: 8, cols: 40 }, config);
    let mut batch = EventBatch::new();

    let start = Instant::now();
    let mut now = start;
    let mut offset = 0;
    for (index, &length) in lengths.iter().cycle().enumerate() {
        if offset >= body.len() {
            break;
        }
        let end = (offset + 1 + usize::from(length) % 64).min(body.len());
        now = start + Duration::from_millis(50 * index as u64);
        black_box(term.feed(&body[offset..end], &mut batch, now));
        read_spans(&batch);
        offset = end;
    }

    let mut state = SnapshotState::new();
    black_box(term.snapshot_update(&mut state, now));
    black_box(term.take_graphics());
});
