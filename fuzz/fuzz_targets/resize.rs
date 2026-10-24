//! Fuzzes `Terminal::resize` interleaved with output, so reflow and anchor tracking see hostile
//! content and hostile sizes (0 rows or columns exercise the clamp).
//!
//! The input is an op stream. A tag byte `t` with `t & 3 == 0` resizes to `rows = b1 % 70` and
//! `cols = b2` (the next two bytes), with the policy from bit 2 of `t`; any other tag feeds the
//! next `t >> 2` bytes. A snapshot update follows every resize.
//! Run: `cargo +nightly-2026-05-25 fuzz run --target-dir target/fuzz-linux resize`

#![no_main]

use std::hint::black_box;
use std::time::{Duration, Instant};

use libfuzzer_sys::fuzz_target;
use oneterm_vt::{Config, EventBatch, ResizePolicy, Size, SnapshotState, Terminal};

fuzz_target!(|data: &[u8]| {
    let config = Config {
        scrollback_limit: 200,
        ..Config::default()
    };
    let mut term = Terminal::new(Size { rows: 24, cols: 80 }, config);
    let mut batch = EventBatch::new();
    let mut state = SnapshotState::new();

    let start = Instant::now();
    let mut step = 0_u64;
    let mut rest = data;
    while let Some((&tag, tail)) = rest.split_first() {
        rest = tail;
        step += 1;
        let now = start + Duration::from_millis(50 * step);
        if tag & 3 == 0 {
            let [rows, cols, tail @ ..] = rest else {
                break;
            };
            rest = tail;
            let policy = if tag & 4 == 0 {
                ResizePolicy::BottomAnchor
            } else {
                ResizePolicy::KeepViewportTop
            };
            let size = Size {
                rows: u16::from(*rows) % 70,
                cols: u16::from(*cols),
            };
            black_box(term.resize(size, policy));
            black_box(term.snapshot_update(&mut state, now));
        } else {
            let (chunk, tail) = rest.split_at(usize::from(tag >> 2).min(rest.len()));
            rest = tail;
            black_box(term.feed(chunk, &mut batch, now));
        }
    }
});
